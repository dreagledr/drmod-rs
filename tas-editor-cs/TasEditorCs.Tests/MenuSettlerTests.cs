using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;

namespace TasEditorCs.Tests;

/// Getting the game out of a menu before a run — the port of the python tools' `ensure_gameplay` /
/// `recover_fail` (`drmod_api` in the sibling repo).
///
/// The steps are mod scripts, so what matters is what the editor *sends* and how it reads the answer:
/// the socket here plays the mod, and its status moves from one menu to the next as scripts come in.
/// The three properties the port exists for are that a running game is left alone, that the pause and
/// fail menus are pressed out with the same one-command scripts the tools send, and that a menu this
/// client does not know is refused rather than guessed at.
public class MenuSettlerTests
{
    [Fact]
    public async Task A_game_already_playing_is_left_alone()
    {
        using var mod = new MenuMod("In Game");
        using var api = new ModApi(mod.Url);

        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.Null(stuck);
        // Nothing was *run*: the only traffic is the state read the step is decided by, and the whole
        // point is not to disturb a game that is fine.
        Assert.Empty(mod.Runs);
        Assert.All(mod.Requests, request => Assert.Equal("/state", request.Path));
    }

    [Fact]
    public async Task The_pause_menu_is_toggled_shut_by_a_script()
    {
        // Closing the pause menu is a `pause` bit — the same three-frame script the tools run. The bit
        // is what the pause menu reads; a keystroke would not reach it (`docs/API.md` §5).
        using var mod = new MenuMod("Pause Menu");
        mod.ScriptPlays("close-menu");
        using var api = new ModApi(mod.Url);

        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.Null(stuck);
        var sent = Assert.Single(mod.Runs);
        // The body is the editor's own write of a script: indented (see the stub's note on the space).
        Assert.Contains("\"name\": \"close-menu\"", sent);
        Assert.Contains("\"pause\": true", sent);
        // Three frames: one frame is what the poll can miss. The step has nothing to trigger on and
        // nothing to restart — it is played the moment the mod takes it.
        Assert.Contains("\"duration\": 3", sent);
        Assert.DoesNotContain("trigger", sent);
        Assert.DoesNotContain("restart", sent);
    }

    [Fact]
    public async Task The_fail_menu_is_left_by_one_confirm()
    {
        // Retry is preselected in the fail menu, so a single confirm is the whole way out (measured in
        // the tools). The statuses the game goes through on the way are not read here — only the one
        // the wait is about.
        using var mod = new MenuMod("Mission Fail");
        mod.ScriptPlays("fail-retry");
        using var api = new ModApi(mod.Url);

        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.Null(stuck);
        var sent = Assert.Single(mod.Runs);
        Assert.Contains("\"name\": \"fail-retry\"", sent);
        Assert.Contains("\"confirm\": true", sent);
        Assert.DoesNotContain("\"pause\": true", sent);
    }

    [Fact]
    public async Task A_menu_this_client_does_not_drive_is_refused_by_name()
    {
        // The front end is nobody's blind confirm: the message names the state so the author knows
        // what to do about it.
        using var mod = new MenuMod("Main Menu");
        using var api = new ModApi(mod.Url);

        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.NotNull(stuck);
        Assert.Contains("Main Menu", stuck);
        Assert.Empty(mod.Runs);
    }

    [Fact]
    public async Task A_mission_that_is_still_loading_is_refused_by_name()
    {
        using var mod = new MenuMod("Loading Into Mission");
        using var api = new ModApi(mod.Url);

        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.NotNull(stuck);
        Assert.Contains("Loading Into Mission", stuck);
        Assert.Empty(mod.Runs);
    }

    [Fact]
    public async Task A_menu_that_does_not_close_says_so_instead_of_arming_the_run()
    {
        // The script is sent, the game stays where it was: the run has to be refused, not started on
        // top of a menu that would swallow its keys.
        using var mod = new MenuMod("Pause Menu");
        using var api = new ModApi(mod.Url);

        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.NotNull(stuck);
        Assert.Contains("Pause Menu", stuck);
        Assert.Single(mod.Runs);
    }

    [Fact]
    public async Task A_held_script_slot_is_cleared_before_the_menu_step()
    {
        // The mod holds one script slot. The caller has already refused to run anything, so a slot
        // that is taken can only be somebody else's script — stopped once, then the step is sent.
        using var mod = new MenuMod("Pause Menu");
        mod.HoldSlot();
        mod.ScriptPlays("close-menu");
        using var api = new ModApi(mod.Url);

        Assert.Null(await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None));
        Assert.Contains(mod.Requests, request => request.Path == "/script/stop");
    }

    [Fact]
    public async Task No_answer_during_the_settle_is_reported_rather_than_assumed()
    {
        // Half a second of silence is the game blocking its render loop; being told "no answer" is how
        // the panel avoids inventing a menu state.
        using var mod = new MenuMod("Pause Menu");
        var url = mod.Url;
        mod.Dispose();

        using var api = new ModApi(url);
        var stuck = await MenuSettler.EnsureGameplayAsync(api, CancellationToken.None);

        Assert.NotNull(stuck);
        Assert.Contains("stopped answering", stuck);
    }

    /// A socket that plays the mod: it answers `/state` with whatever menu it is told to be in, and
    /// moves to `In Game` once the script whose name it was told about comes in.
    sealed class MenuMod : IDisposable
    {
        static readonly byte[] Separator = "\r\n\r\n"u8.ToArray();

        readonly TcpListener _listener;
        readonly Thread _worker;
        readonly List<(string Method, string Path, string Body)> _requests = [];
        readonly List<string> _runs = [];
        readonly Lock _lock = new();
        string _current;
        string? _step;
        bool _slotHeld;
        volatile bool _stopping;

        internal MenuMod(string menu)
        {
            _current = menu;
            _listener = new TcpListener(IPAddress.Loopback, 0);
            _listener.Start();
            Port = ((IPEndPoint)_listener.LocalEndpoint).Port;
            _worker = new Thread(Serve) { IsBackground = true };
            _worker.Start();
        }

        internal int Port { get; }

        internal string Url => $"http://127.0.0.1:{Port}";

        internal IReadOnlyList<(string Method, string Path, string Body)> Requests
        {
            get
            {
                lock (_lock) return [.. _requests];
            }
        }

        /// The script bodies that were run, in order.
        internal IReadOnlyList<string> Runs
        {
            get
            {
                lock (_lock) return [.. _runs];
            }
        }

        /// Makes `/script/run` answer `409` once — the mod's one slot being taken.
        internal void HoldSlot() => _slotHeld = true;

        /// The name of the step the game is waiting for. Named rather than assumed because the two
        /// steps are told apart by it in the sent body — the same thing the mod reads.
        internal void ScriptPlays(string name) => _step = name;

        public void Dispose()
        {
            _stopping = true;
            _listener.Stop();
            _worker.Join(TimeSpan.FromSeconds(2));
        }

        void Serve()
        {
            while (!_stopping)
            {
                TcpClient client;
                try
                {
                    client = _listener.AcceptTcpClient();
                }
                catch (Exception)
                {
                    return;
                }

                using (client)
                {
                    Reply(client);
                }
            }
        }

        void Reply(TcpClient client)
        {
            var stream = client.GetStream();
            var received = new List<byte>();
            var chunk = new byte[1024];

            var separator = -1;
            while (separator < 0)
            {
                var read = stream.Read(chunk, 0, chunk.Length);
                if (read <= 0) return;
                received.AddRange(chunk[..read]);
                separator = IndexOf(received, Separator);
            }

            var lines = Encoding.ASCII.GetString(received.GetRange(0, separator).ToArray()).Split("\r\n");
            var request = lines[0].Split(' ');
            var method = request[0];
            var path = request[1];

            var length = 0;
            foreach (var line in lines[1..])
            {
                var field = line.Split(':', 2);
                if (field.Length == 2 && field[0].Trim().Equals("Content-Length", StringComparison.OrdinalIgnoreCase))
                {
                    length = int.Parse(field[1].Trim());
                }
            }

            var start = separator + Separator.Length;
            while (received.Count - start < length)
            {
                var read = stream.Read(chunk, 0, chunk.Length);
                if (read <= 0) return;
                received.AddRange(chunk[..read]);
            }

            var body = Encoding.UTF8.GetString(received.GetRange(start, length).ToArray());

            int code;
            string payload;
            lock (_lock)
            {
                _requests.Add((method, path, body));

                switch (path)
                {
                    case "/state":
                        code = 200;
                        payload = State();
                        break;
                    case "/script/stop":
                        _slotHeld = false;
                        code = 200;
                        payload = """{"stopped":true}""";
                        break;
                    case "/script/run" when _slotHeld:
                        code = 409;
                        payload = """{"error":"script already active: id=7 name=other"}""";
                        break;
                    case "/script/run":
                        _runs.Add(body);
                        // The step is *played* the moment the mod takes it (it has no trigger), so the
                        // menu is settled by the time the first status read after it comes in. What the
                        // stub does not model is a menu that stays up — which is exactly why the step's
                        // own wait for `In Game` has no test here.
                        //
                        // ⚠️ The name is matched with the space: the editor writes indented JSON
                        // (`WriteIndented` in `ScriptJsonContext`), so it is `"name": "x"` and not
                        // `"name":"x"` — a substring test that assumes the compact spelling silently
                        // never fires.
                        if (_step is { } step && body.Contains($"\"name\": \"{step}\""))
                        {
                            _current = MenuSettler.InGame;
                        }

                        code = 200;
                        payload = ScriptAccepted(body, "done");
                        break;
                    default:
                        code = 200;
                        payload = "{}";
                        break;
                }
            }

            var bytes = Encoding.UTF8.GetBytes(payload);
            var head = $"HTTP/1.1 {code} drmod\r\n"
                + "Content-Type: application/json; charset=utf-8\r\n"
                + $"Content-Length: {bytes.Length}\r\n"
                + "Connection: close\r\n\r\n";

            stream.Write(Encoding.ASCII.GetBytes(head));
            stream.Write(bytes);
            stream.Flush();
        }

        /// The mod's `/state`, trimmed to what the settler reads. `script` is absent: an armed
        /// one-command script is over by the time anyone asks.
        string State() =>
            $$"""
            {
              "mission_id": 272,
              "mission_name": "R-03",
              "menu_status": "{{_current}}",
              "fps": 58.4,
              "dt": { "fixed": true, "fixed_ms": 16.666668, "frame_ms": 16.75, "frames": 1 },
              "rng_pin": "freeze",
              "rng_seed": 1,
              "fps_cap": { "cap": "game", "limit": 60 },
              "render": { "headless": false, "hold": false }
            }
            """;

        static string ScriptAccepted(string body, string status)
        {
            var name = "step";
            var at = body.IndexOf("\"name\":\"", StringComparison.Ordinal);
            if (at >= 0)
            {
                var from = at + 8;
                var to = body.IndexOf('"', from);
                if (to > from) name = body[from..to];
            }

            return $$"""{"script_id":99,"name":"{{name}}","total_frames":3,"status":"{{status}}"}""";
        }

        static int IndexOf(List<byte> data, byte[] pattern)
        {
            for (var at = 0; at + pattern.Length <= data.Count; at++)
            {
                var found = true;
                for (var offset = 0; offset < pattern.Length; offset++)
                {
                    if (data[at + offset] == pattern[offset]) continue;
                    found = false;
                    break;
                }

                if (found) return at;
            }

            return -1;
        }
    }
}
