using System;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

/// The mod's HTTP API as the editor uses it: the state it reads, the script it starts, and the four
/// run levers it sets.
///
/// A request comes back as a value, never as an exception — the same rule the workspace follows
/// (`Workspace`), and for the same reason: the only thing a click handler or a poll tick can do
/// with a failure is paint it. The three ways a call can end are therefore spelled out in
/// <see cref="ApiResult{T}"/>: an answer, a refusal by the mod (with the mod's own wording, which
/// already names the field), and no answer at all.
///
/// The transport is `HttpClient`, and every request carries `Connection: close`: the mod's server
/// answers and then closes the socket (`respond` in `src/api.rs`), so the connection is not handed
/// back to a pool to be reused after the server has already dropped it. A dropped connection under
/// load is retried once, like the python tools do (`drmod_api.http`).
///
/// ⚠️ The mod's server is single-threaded and lives in the game's render loop: it answers one
/// request at a time. Polling it is cheap but not free, which is why the panel asks a couple of
/// times a second and never per frame (`Editor`).
internal sealed class ModApi : IDisposable
{
    /// Where the mod listens (`src/api.rs`, both builds).
    internal const string DefaultUrl = "http://127.0.0.1:5223";

    /// One retry, not a loop: the second failure is the answer.
    const int Retries = 1;

    static readonly TimeSpan RetryDelay = TimeSpan.FromMilliseconds(200);

    readonly HttpClient _http;

    internal ModApi(string url = DefaultUrl)
    {
        var handler = new SocketsHttpHandler
        {
            ConnectTimeout = TimeSpan.FromSeconds(1.5),
        };

        _http = new HttpClient(handler)
        {
            BaseAddress = new Uri(url),
            // A request is answered in milliseconds; the wait is for the game's own stalls (a level
            // loading blocks the render loop the server runs in). Longer than this and the poll
            // would lag behind the run it is reporting on.
            Timeout = TimeSpan.FromSeconds(2.5),
        };
    }

    /// `GET /state` — the whole snapshot, as the panel's model.
    internal async Task<ApiResult<GameStatus>> StateAsync(CancellationToken ct)
    {
        var answer = await GetAsync("/state", ct);
        if (!answer.Ok)
        {
            return ApiResult<GameStatus>.From(answer);
        }

        var state = ApiJson.ReadState(answer.Value!);
        return state is null
            ? ApiResult<GameStatus>.Failed("the mod's /state did not read as JSON")
            : ApiResult<GameStatus>.Good(GameStatus.Of(state));
    }

    /// `POST /script/run` — the script body is the editor's own JSON (`ScriptJson.Write`), which is
    /// the same shape the fixtures carry, so the mod's own parser is the only authority on it.
    ///
    /// A `409` means the mod's one script slot is taken: it is reported as its own kind of answer,
    /// because the caller can do something about it (stop that script and run this one).
    internal async Task<ApiResult<ApiJson.RunResponse>> RunAsync(string script, CancellationToken ct)
    {
        var answer = await PostAsync("/script/run", script, ct);
        if (!answer.Ok)
        {
            return ApiResult<ApiJson.RunResponse>.From(answer);
        }

        var run = ApiJson.ReadRun(answer.Value!);
        return run is null
            ? ApiResult<ApiJson.RunResponse>.Failed("the mod's /script/run did not read as JSON")
            : ApiResult<ApiJson.RunResponse>.Good(run);
    }

    /// `POST /script/stop` — clears the override and frees the slot. The mod restores the render and
    /// the frame cap by itself when a headless run ends, cancelled or not (`headless_service`).
    internal Task<ApiResult<string>> StopAsync(CancellationToken ct) =>
        PostAsync("/script/stop", null, ct);

    /// `POST /dt`, `/fps`, `/rng`, `/render` — the levers. The bodies are built by
    /// <see cref="PlaybackRules"/>; what comes back is only told to be an answer or not.
    internal Task<ApiResult<string>> PostAsync(string path, string? body, CancellationToken ct) =>
        Send(HttpMethod.Post, path, body, ct);

    internal Task<ApiResult<string>> GetAsync(string path, CancellationToken ct) =>
        Send(HttpMethod.Get, path, null, ct);

    async Task<ApiResult<string>> Send(HttpMethod method, string path, string? body, CancellationToken ct)
    {
        for (var attempt = 0; ; attempt++)
        {
            try
            {
                // A fresh request per attempt: a request message that has been sent once cannot be
                // sent again.
                using var request = new HttpRequestMessage(method, path);
                // The mod's server answers and closes (`respond` in `src/api.rs`), so the request says
                // so too and the connection is not handed back to a pool to be reused after the server
                // has dropped it.
                request.Headers.ConnectionClose = true;
                if (body is not null)
                {
                    request.Content = new StringContent(body, Encoding.UTF8, "application/json");
                }

                using var response = await _http.SendAsync(request, ct);
                var payload = await response.Content.ReadAsStringAsync(ct);

                if (response.IsSuccessStatusCode)
                {
                    return ApiResult<string>.Good(payload);
                }

                var refused = ApiJson.ReadError(payload) ?? $"HTTP {(int)response.StatusCode}";
                return response.StatusCode == HttpStatusCode.Conflict
                    ? ApiResult<string>.Taken(refused)
                    : ApiResult<string>.Failed(refused);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                // Whoever cancelled is the one who decides what that means (the poll loop stops, a
                // closing window lets its client go).
                throw;
            }
            catch (Exception error) when (error is HttpRequestException or TaskCanceledException or IOException)
            {
                if (attempt >= Retries)
                {
                    return ApiResult<string>.NoAnswer(error.Message);
                }

                await Task.Delay(RetryDelay, ct);
            }
        }
    }

    public void Dispose() => _http.Dispose();
}

/// How a call to the mod ended: an answer, a refusal, or no answer at all.
///
/// `T` is the parsed answer; the raw payload is what a lever's call gets, since the editor only
/// needs to know it went through.
internal readonly record struct ApiResult<T>(T? Value, string? Error, bool Offline, bool Conflict)
    where T : class
{
    /// The mod answered and the body parsed.
    internal bool Ok => Value is not null && Error is null;

    /// What to show the user: the mod's own wording when it refused, and otherwise why there was
    /// no answer at all.
    internal string Message => Error ?? "the mod did not answer";

    internal static ApiResult<T> Good(T value) => new(value, null, Offline: false, Conflict: false);

    internal static ApiResult<T> Failed(string error) => new(null, error, Offline: false, Conflict: false);

    /// The mod refused because its one script slot is taken (`409`) — an answer the caller can do
    /// something about, rather than a plain failure.
    internal static ApiResult<T> Taken(string error) => new(null, error, Offline: false, Conflict: true);

    /// No answer: nothing is injected, the game is not running, or the mod dropped the socket twice.
    internal static ApiResult<T> NoAnswer(string error) => new(null, error, Offline: true, Conflict: false);

    /// The same outcome, read as another answer type.
    internal static ApiResult<T> From<TFrom>(ApiResult<TFrom> result) where TFrom : class =>
        new(null, result.Error, result.Offline, result.Conflict);
}
