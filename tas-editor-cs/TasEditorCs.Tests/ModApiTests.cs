using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;

namespace TasEditorCs.Tests;

/// The mod's HTTP API as the editor speaks it.
///
/// The contract is a program in another repository, so it is pinned against a **real socket** that
/// answers the way the mod's own server does — `Connection: close`, `Content-Length`, JSON bodies
/// (`respond` in `src/api.rs`). What the tests fake is the game, not the transport: a request that
/// leaves with the wrong field name, or a refusal that loses the mod's own wording, would pass
/// against a mock and fail against the game.
public class ModApiTests
{
    /// A `/state` as the mod writes one — trimmed to the fields the panel reads, which is exactly
    /// what a client ignoring unknown keys is for.
    const string State = """
    {
      "t_ms": 123456,
      "mission_id": 272,
      "mission_name": "R-03",
      "menu_status": "In Game",
      "player": { "found": true, "pos": [-24.7, 12.1, 120.7], "hp": 100 },
      "camera": { "pos": [-24.7, 12.1, 120.7], "look_at": [-24.7, 12.1, 119.7], "rot": [1.5708, 0, 0] },
      "script": { "id": 7, "name": "r03-barrier-ticks", "status": "running", "frame": 412, "total_frames": 1300 },
      "fps": 58.4,
      "sim_ticks": 900,
      "dt": { "fixed": true, "fixed_ms": 16.666668, "frame_ms": 16.75, "rate": 1.005, "frames": 41, "synth_ticks_ms": 683.0, "synth_base_ms": 0.0, "ticks": true },
      "rng_pin": "freeze",
      "rng_seed": 1,
      "cutscene_skip": "off",
      "fps_cap": { "cap": "game", "limit": 60 },
      "render": { "skip_overlay": false, "skip_present": false, "skip_draw": false, "draw_hooked": false, "headless": false, "hold": false }
    }
    """;

    [Fact]
    public async Task The_snapshot_comes_back_as_the_panels_model()
    {
        using var mod = new StubMod();
        mod.Answer("/state", 200, State);
        using var api = new ModApi(mod.Url);

        var answer = await api.StateAsync(CancellationToken.None);

        Assert.True(answer.Ok);
        var game = answer.Value!;
        Assert.True(game.Online);
        Assert.True(game.InGameplay);
        Assert.Equal("In Game", game.MenuStatus);
        Assert.Equal("R-03", game.MissionName);
        Assert.Equal(272, game.MissionId);
        Assert.Equal(58.4f, game.Fps);
        Assert.True(game.FixedTick);
        Assert.Equal(60u, game.CapLimit);
        Assert.Equal("freeze", game.RngPin);
        Assert.Equal(1u, game.RngSeed);
        Assert.False(game.Headless);

        Assert.True(game.ScriptActive);
        var script = game.Script!;
        Assert.Equal(7u, script.Id);
        Assert.Equal("r03-barrier-ticks", script.Name);
        Assert.Equal(GameScriptPhase.Running, script.Phase);
        Assert.Equal(412u, script.Frame);
        Assert.Equal(1300u, script.TotalFrames);
    }

    [Fact]
    public async Task A_status_the_editor_does_not_know_still_reads_as_a_snapshot()
    {
        // A word the mod learns later must cost the panel one field, not the whole status line — and an
        // unknown word is not an active script, which is what the buttons are decided by.
        using var mod = new StubMod();
        mod.Answer("/state", 200, State.Replace("\"running\"", "\"rewinding\""));
        using var api = new ModApi(mod.Url);

        var game = (await api.StateAsync(CancellationToken.None)).Value!;

        Assert.True(game.Online);
        Assert.Equal(GameScriptPhase.Unknown, game.Script!.Phase);
        Assert.False(game.ScriptActive);
    }

    [Fact]
    public async Task A_menu_is_not_gameplay()
    {
        // A run needs gameplay: the script's own restart plays the pause menu, and a menu that is
        // already open would swallow the keys.
        using var mod = new StubMod();
        mod.Answer("/state", 200, State.Replace("\"In Game\"", "\"Pause Menu\""));
        using var api = new ModApi(mod.Url);

        var game = (await api.StateAsync(CancellationToken.None)).Value!;

        Assert.True(game.Online);
        Assert.False(game.InGameplay);
    }

    [Fact]
    public async Task A_refusal_keeps_the_mods_own_wording()
    {
        using var mod = new StubMod();
        mod.Refuse("/script/run", 400, "name too long (max 64)");
        using var api = new ModApi(mod.Url);

        var answer = await api.RunAsync("{}", CancellationToken.None);

        Assert.False(answer.Ok);
        Assert.False(answer.Offline);
        Assert.False(answer.Conflict);
        Assert.Equal("name too long (max 64)", answer.Message);
    }

    [Fact]
    public async Task A_taken_script_slot_is_its_own_answer()
    {
        // 409 is the one refusal the caller can act on — stop that script and run this one.
        using var mod = new StubMod();
        mod.Refuse("/script/run", 409, "script already active: id=7 name=r03");
        using var api = new ModApi(mod.Url);

        var answer = await api.RunAsync("{}", CancellationToken.None);

        Assert.True(answer.Conflict);
        Assert.False(answer.Ok);
        Assert.Equal("script already active: id=7 name=r03", answer.Message);
    }

    [Fact]
    public async Task Nothing_answering_is_no_answer_rather_than_a_refusal()
    {
        // Nothing injected, or the game not running at all: the panel says "offline" rather than
        // pretending the mod turned a request down.
        var mod = new StubMod();
        var url = mod.Url;
        mod.Dispose();

        using var api = new ModApi(url);
        var answer = await api.StateAsync(CancellationToken.None);

        Assert.False(answer.Ok);
        Assert.True(answer.Offline);
        Assert.False(string.IsNullOrEmpty(answer.Message));
    }

    [Fact]
    public async Task A_run_hands_the_body_over_and_reads_the_answer_back()
    {
        using var mod = new StubMod();
        mod.Answer("/script/run", 200,
            """{"script_id": 8, "name": "r03-barrier-ticks", "total_frames": 1300, "status": "armed"}""");
        using var api = new ModApi(mod.Url);

        var started = await api.RunAsync("""{"name":"probe"}""", CancellationToken.None);

        Assert.True(started.Ok);
        Assert.Equal(8u, started.Value!.ScriptId);
        Assert.Equal(1300u, started.Value.TotalFrames);
        Assert.Equal("armed", started.Value.Status);

        var sent = Assert.Single(mod.Requests);
        Assert.Equal("POST", sent.Method);
        Assert.Equal("/script/run", sent.Path);
        Assert.Equal("""{"name":"probe"}""", sent.Body);
    }

    [Fact]
    public async Task The_rules_are_applied_in_the_order_a_run_needs_them()
    {
        // The tick and the cap first, the seed **last**: the mod freezes the LCG on the first tick of
        // the *next* script, so a pin applied any earlier would land on nothing.
        using var mod = new StubMod();
        using var api = new ModApi(mod.Url);

        var error = await PlaybackRules.Default.ApplyAsync(api, CancellationToken.None);

        Assert.Null(error);
        Assert.Equal(["/dt", "/fps", "/rng"], mod.Requests.Select(request => request.Path));
        Assert.Equal("""{"fixed":true,"ticks":true}""", mod.Requests[0].Body);
        Assert.Equal("""{"cap":"game"}""", mod.Requests[1].Body);
        Assert.Equal("""{"pin":"freeze","seed":1}""", mod.Requests[2].Body);
    }

    [Fact]
    public async Task A_lever_the_mod_refuses_stops_the_run_before_the_script()
    {
        using var mod = new StubMod();
        mod.Refuse("/fps", 400, "fps=0 out of range 1..1000");
        using var api = new ModApi(mod.Url);

        var error = await PlaybackRules.Default.ApplyAsync(api, CancellationToken.None);

        Assert.Equal("/fps: fps=0 out of range 1..1000", error);
        // The seed was never pinned: a run whose levers were refused would have gone out with the
        // wrong configuration, and the seed's own place in the order is why it is still unsent.
        Assert.Equal(["/dt", "/fps"], mod.Requests.Select(request => request.Path));
    }

    [Fact]
    public async Task The_headless_switch_and_the_way_back_are_the_mods_own_bodies()
    {
        using var mod = new StubMod();
        using var api = new ModApi(mod.Url);

        await api.PostAsync("/render", ApiJson.Headless(true), CancellationToken.None);
        await api.PostAsync("/render", ApiJson.Reset(), CancellationToken.None);

        Assert.Equal("""{"headless":true}""", mod.Requests[0].Body);
        Assert.Equal("""{"reset":true}""", mod.Requests[1].Body);
    }

    [Fact]
    public async Task Stopping_asks_the_mod_to_clear_the_slot()
    {
        using var mod = new StubMod();
        mod.Answer("/script/stop", 200, """{"stopped": true, "script_id": 7}""");
        using var api = new ModApi(mod.Url);

        var stopped = await api.StopAsync(CancellationToken.None);

        Assert.True(stopped.Ok);
        var sent = Assert.Single(mod.Requests);
        Assert.Equal("POST", sent.Method);
        Assert.Equal("/script/stop", sent.Path);
        Assert.Equal(string.Empty, sent.Body);
    }

    /// A socket that answers like the mod's own server: `Connection: close`, `Content-Length`, JSON,
    /// one request per connection. Every request is kept, so a test can assert what left and in which
    /// order — which is the half a fake `ModApi` could not pin.
    sealed class StubMod : IDisposable
    {
        static readonly byte[] Separator = "\r\n\r\n"u8.ToArray();

        readonly TcpListener _listener;
        readonly Thread _worker;
        readonly Dictionary<string, (int Code, string Body)> _answers = new(StringComparer.Ordinal);
        readonly List<(string Method, string Path, string Body)> _requests = [];
        readonly Lock _lock = new();
        volatile bool _stopping;

        internal StubMod()
        {
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

        internal void Answer(string path, int code, string body)
        {
            lock (_lock) _answers[path] = (code, body);
        }

        internal void Refuse(string path, int code, string error) =>
            Answer(path, code, $$"""{"error":"{{error}}"}""");

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
                    // The listener was stopped from `Dispose` — that is the way out of this loop.
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
            lock (_lock) _requests.Add((method, path, body));

            (int Code, string Body) answer;
            lock (_lock)
            {
                answer = _answers.TryGetValue(path, out var found) ? found : (200, "{}");
            }

            var payload = Encoding.UTF8.GetBytes(answer.Body);
            var head = $"HTTP/1.1 {answer.Code} drmod\r\n"
                + "Content-Type: application/json; charset=utf-8\r\n"
                + $"Content-Length: {payload.Length}\r\n"
                + "Connection: close\r\n\r\n";

            stream.Write(Encoding.ASCII.GetBytes(head));
            stream.Write(payload);
            stream.Flush();
        }

        static int IndexOf(List<byte> data, byte[] pattern)
        {
            for (var start = 0; start + pattern.Length <= data.Count; start++)
            {
                var found = true;
                for (var offset = 0; offset < pattern.Length; offset++)
                {
                    if (data[start + offset] == pattern[offset]) continue;
                    found = false;
                    break;
                }

                if (found) return start;
            }

            return -1;
        }
    }
}
