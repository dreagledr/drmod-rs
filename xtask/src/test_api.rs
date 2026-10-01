//! Package-level smoke tests for the mod's HTTP API (was `test_api.ps1`).

use std::path::PathBuf;
use std::time::Duration;

use drmod_replay_types::script::{ScriptCommand, ScriptInput, ScriptRequest, ScriptTrigger};

use crate::{step, util, Error, Result};

const USAGE: &str = "\
Usage: cargo xtask test-api [options]

Smoke-tests the mod's HTTP API on 127.0.0.1:5223. The game must be running with the mod injected.

Options:
  --base-url <url>     API root (default: http://127.0.0.1:5223)
  --eject              End with POST /eject — unloads the DLL and releases the port
  -h, --help           Show this message

Exits 0 when every check passed, 1 otherwise.

Requires the game running with the mod injected (the API listens on 127.0.0.1:5223). The steps are
the PowerShell script's, one for one: health, state, a script run, its status, the log ring, stop, an
armed (position-triggered) run, the error paths, the headless switch, a load test, and — with --eject
— unloading the DLL.

WHAT CHANGED IN THE PORT

The request bodies are built out of the mod's own DTOs (drmod-replay-types::script) instead of
hand-written JSON. That is the difference that matters: the mod's accepted format is a type, so a
test that serialises that type cannot drift away from it, while a test holding a JSON string can only
be as right as the last time somebody read the mod's parser.

⚠️ Every request carries a timeout. The API is a hand-rolled single-threaded server inside the game's
render loop: when the game is paused, alt-tabbed or mid-load it stops answering, and a client without
a timeout hangs forever instead of reporting that.
";

/// How long any single request may take. See the module docs: the server answers on the game's render
/// thread, so a stalled game must surface as a failed check, not as a hung tool.
const TIMEOUT: Duration = Duration::from_secs(3);
/// The load test fires this many requests at once.
const PARALLEL_REQUESTS: usize = 20;

/// Counts failed checks so the summary can report them; the names are what makes the summary useful
/// when something did fail.
struct Report {
    failures: Vec<String>,
}

impl Report {
    fn new() -> Self {
        Self { failures: Vec::new() }
    }

    fn pass(&self, name: &str) {
        println!("  PASS: {name}");
    }

    fn fail(&mut self, name: &str) {
        println!("  FAIL: {name}");
        self.failures.push(name.to_owned());
    }

    /// A check on a value, reported in the shape the PowerShell script used: expected beside actual.
    fn equal<T: PartialEq + std::fmt::Debug>(&mut self, name: &str, actual: &T, expected: &T) {
        if actual == expected {
            self.pass(name);
        } else {
            self.fail(&format!("{name} (expected {expected:?}, got {actual:?})"));
        }
    }

    fn is_true(&mut self, name: &str, condition: bool) {
        if condition {
            self.pass(name);
        } else {
            self.fail(name);
        }
    }

    /// Records a whole step that could not run — a connection error, a body that would not parse.
    fn error(&mut self, step: &str, error: impl std::fmt::Display) {
        println!("  FAIL: {step}: {error}");
        self.failures.push(step.to_owned());
    }

    fn summary(&self) -> bool {
        println!();
        step("Summary");
        if self.failures.is_empty() {
            println!("All tests passed!");
            true
        } else {
            println!("{} check(s) failed", self.failures.len());
            for name in &self.failures {
                println!("  - {name}");
            }
            false
        }
    }
}

pub fn run(root: PathBuf, args: &[String]) -> Result<()> {
    crate::announce(&root);

    let mut base_url = "http://127.0.0.1:5223".to_owned();
    let mut eject = false;

    let mut args = util::Args::new(args);
    while let Some(arg) = args.next() {
        match arg {
            "--base-url" => base_url = args.value("--base-url")?,
            "--eject" => eject = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(Error::new(format!("unknown option `{other}`\n\n{USAGE}"))),
        }
    }

    let api = Api::new(base_url.trim_end_matches('/'))?;
    let mut report = Report::new();

    if !health(&api, &mut report) {
        // The first step failing means there is nothing to test against: saying so is more useful
        // than twenty more failures that all mean "no mod".
        println!();
        return Err(Error::new(format!(
            "the API at {} is unreachable.\n       \
             Make sure the game is running and the mod is injected \
             (`cargo xtask build`, then run out/drmod.exe or install the ASI package).",
            api.base_url
        )));
    }

    state(&api, &mut report);

    let script_id = run_script(&api, &mut report);
    if let Some(id) = &script_id {
        script_status(&api, &mut report, id);
        logs(&api, &mut report, id);
    }

    stop_script(&api, &mut report, None);

    let armed_id = run_armed_script(&api, &mut report);
    if let Some(id) = &armed_id {
        armed_status(&api, &mut report, id);
        stop_script(&api, &mut report, Some(id));
    }

    error_paths(&api, &mut report);
    headless(&api, &mut report);
    load_test(&api, &mut report);

    if eject {
        eject_check(&api, &mut report);
    }

    if report.summary() {
        Ok(())
    } else {
        Err(Error::new("the smoke test reported failures (see above)"))
    }
}

/// A blocking HTTP client with the timeout the module docs explain.
struct Api {
    agent: ureq::Agent,
    base_url: String,
}

impl Api {
    fn new(base_url: &str) -> Result<Self> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build();

        Ok(Self {
            agent: config.into(),
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    /// `GET path`, with the body read as JSON.
    ///
    /// One method rather than a request-then-parse pair, because every caller wants both and the
    /// failure that matters — a game that stopped answering — is the same in each case.
    fn get_json(&self, path: &str) -> std::result::Result<serde_json::Value, String> {
        let response = self
            .agent
            .get(format!("{}{path}", self.base_url))
            .call()
            .map_err(|error| describe(&error))?;

        read_json(response.into_body())
    }

    /// `POST path` with a JSON body, with the response body read as JSON.
    fn post_json(&self, path: &str, body: &str) -> std::result::Result<serde_json::Value, String> {
        let response = self
            .agent
            .post(format!("{}{path}", self.base_url))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|error| describe(&error))?;

        read_json(response.into_body())
    }

    /// `POST path` for the status code alone — used where the response body is not what is checked.
    fn post(&self, path: &str, body: &str) -> std::result::Result<(), String> {
        self.agent
            .post(format!("{}{path}", self.base_url))
            .header("Content-Type", "application/json")
            .send(body)
            .map(|_| ())
            .map_err(|error| describe(&error))
    }

    /// `GET path` for the status code alone.
    fn get(&self, path: &str) -> std::result::Result<(), String> {
        self.agent
            .get(format!("{}{path}", self.base_url))
            .call()
            .map(|_| ())
            .map_err(|error| describe(&error))
    }
}

fn read_json(mut body: ureq::Body) -> std::result::Result<serde_json::Value, String> {
    let text = body
        .read_to_string()
        .map_err(|error| format!("cannot read the response body: {error}"))?;

    serde_json::from_str(&text).map_err(|error| format!("the response is not JSON: {error}"))
}

/// `ureq`'s error, as the text a failed check shows. The status code is spelled out when there is
/// one, because "HTTP 404" is the whole story for half of the error-path checks.
fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => format!("HTTP {code}"),
        other => other.to_string(),
    }
}

/// The status code `ureq` reported, for the checks that *expect* a 4xx.
fn status_of(error: &str) -> Option<u16> {
    error.strip_prefix("HTTP ")?.parse().ok()
}

/// Step 1 — `/health`. Returns whether the API answered at all.
fn health(api: &Api, report: &mut Report) -> bool {
    println!();
    step("Step 1: GET /health");

    let value = match api.get_json("/health") {
        Ok(value) => value,
        Err(error) => {
            report.error("/health unreachable", error);
            return false;
        }
    };

    report.equal("status", &value["status"], &serde_json::json!("ok"));
    report.is_true("base_addr present", !value["base_addr"].is_null());
    true
}

/// Step 2 — `/state`.
fn state(api: &Api, report: &mut Report) {
    println!();
    step("Step 2: GET /state");

    let value = match api.get_json("/state") {
        Ok(value) => value,
        Err(error) => return report.error("/state unreachable", error),
    };

    report.equal("player.found", &value["player"]["found"], &serde_json::json!(true));

    let mission = value["mission_name"].as_str().unwrap_or_default();
    report.is_true("mission_name non-empty", !mission.is_empty());
}

/// Step 3 — `POST /script/run`, returning the id it assigned.
fn run_script(api: &Api, report: &mut Report) -> Option<String> {
    println!();
    step("Step 3: POST /script/run");

    let request = ScriptRequest {
        name: "smoke-test".to_owned(),
        commands: vec![walk_command()],
        trigger: None,
        restart: None,
    };

    match api.post_json("/script/run", &body(&request, report)?) {
        Ok(value) => {
            let id = value["script_id"].as_str().map(str::to_owned);
            report.is_true("script_id returned", id.is_some());
            report.equal("name", &value["name"], &serde_json::json!("smoke-test"));
            id
        }
        Err(error) => {
            report.error("/script/run", error);
            None
        }
    }
}

/// Step 4 — `GET /script/{id}`.
fn script_status(api: &Api, report: &mut Report, id: &str) {
    println!();
    step(&format!("Step 4: GET /script/{id}"));

    match api.get_json(&format!("/script/{id}")) {
        Ok(value) => {
            report.equal("id", &value["id"], &serde_json::json!(id));
            let status = value["status"].as_str().unwrap_or_default();
            report.is_true(
                "status in (running,done,stopped)",
                matches!(status, "running" | "done" | "stopped"),
            );
        }
        Err(error) => report.error(&format!("/script/{id}"), error),
    }
}

/// Step 5 — `GET /logs?script_id=`.
fn logs(api: &Api, report: &mut Report, id: &str) {
    println!();
    step(&format!("Step 5: GET /logs?script_id={id}"));

    match api.get_json(&format!("/logs?script_id={id}&limit=5")) {
        Ok(value) => {
            report.is_true("count >= 0", value["count"].as_i64().is_some_and(|n| n >= 0));
            report.is_true("frames is array", value["frames"].is_array());
        }
        Err(error) => report.error("/logs", error),
    }
}

/// Step 6 and 9 — `POST /script/stop`, with or without a known armed script.
///
/// With no active script the API answers 404, and that is a pass: it means "nothing to stop", which
/// is exactly the state the test expects for step 6.
fn stop_script(api: &Api, report: &mut Report, armed_id: Option<&str>) {
    println!();
    step(match armed_id {
        Some(_) => "Step 9: POST /script/stop (armed)",
        None => "Step 6: POST /script/stop",
    });

    match api.post_json("/script/stop", "") {
        Ok(value) => {
            report.is_true("stopped response", !value.is_null());
            if let Some(id) = armed_id {
                report.equal("script_id", &value["script_id"], &serde_json::json!(id));
            }
        }
        Err(error) if status_of(&error) == Some(404) && armed_id.is_none() => {
            report.pass("/script/stop (no active script, 404)");
        }
        Err(error) => report.error("/script/stop", error),
    }
}

/// Step 7 — `POST /script/run` with a position trigger, which arms instead of starting.
fn run_armed_script(api: &Api, report: &mut Report) -> Option<String> {
    println!();
    step("Step 7: POST /script/run with trigger");

    let request = ScriptRequest {
        name: "trigger-test".to_owned(),
        commands: vec![walk_command()],
        // Out of the world's reach on purpose: the script must stay armed and never take over.
        trigger: Some(ScriptTrigger {
            pos: Some([99999.0, 99999.0, 99999.0]),
            ticks: None,
        }),
        restart: None,
    };

    match api.post_json("/script/run", &body(&request, report)?) {
        Ok(value) => {
            let id = value["script_id"].as_str().map(str::to_owned);
            report.is_true("script_id returned", id.is_some());
            report.equal("status", &value["status"], &serde_json::json!("armed"));
            id
        }
        Err(error) => {
            report.error("/script/run (trigger)", error);
            None
        }
    }
}

/// Step 8 — `GET /script/{id}` while armed.
fn armed_status(api: &Api, report: &mut Report, id: &str) {
    println!();
    step(&format!("Step 8: GET /script/{id} (armed)"));

    match api.get_json(&format!("/script/{id}")) {
        Ok(value) => {
            report.equal("id", &value["id"], &serde_json::json!(id));
            report.equal("status", &value["status"], &serde_json::json!("armed"));
        }
        Err(error) => report.error(&format!("/script/{id}"), error),
    }
}

/// Step 10 — the error paths: bad JSON, an unknown script, an unknown route, all 4xx.
fn error_paths(api: &Api, report: &mut Report) {
    println!();
    step("Step 10: error paths");

    let cases: [(&str, &str, bool, u16); 3] = [
        ("invalid JSON", "/script/run", true, 400),
        ("unknown script", "/script/999", false, 404),
        ("unknown route", "/nope", false, 404),
    ];

    for (name, path, is_post, expected) in cases {
        let result = if is_post {
            api.post(path, "not json")
        } else {
            api.get(path)
        };

        // A 4xx is an *error* to `ureq` but the *expected* answer here, so the status is read off the
        // error rather than treated as a failure.
        match result {
            Err(error) if status_of(&error) == Some(expected) => {
                report.pass(&format!("{name} -> {expected}"));
            }
            Err(error) => report.error(&format!("{name} (expected {expected})"), error),
            Ok(()) => report.fail(&format!("{name} should be {expected}")),
        }
    }
}

/// Step 11 — the headless switch (`POST /render`).
///
/// Production headless takes both the overlay and the game geometry and lifts the frame cap; `reset`
/// puts both back, including the cap that was in force before. Present headless deliberately does
/// **not** take: no gain, and combined with `skip_draw` it crashes the game (`docs/HEADLESS.md` §5).
/// The granular switch is checked separately because it must go past the headless run without
/// touching the cap.
fn headless(api: &Api, report: &mut Report) {
    println!();
    step("Step 11: POST /render (headless)");

    let before = match api.get_json("/state") {
        Ok(value) => value,
        Err(error) => return report.error("/state before headless", error),
    };

    report.is_true("state.render present", !before["render"].is_null());
    report.equal("render baseline off", &before["render"]["headless"], &serde_json::json!(false));
    let cap_before = before["fps_cap"]["cap"].clone();

    // A closure so the "whatever failed, leave rendering back on" tail is written once: a headless
    // game looks frozen, and the person reading the failure should not have to reload it by hand.
    let outcome = (|| -> std::result::Result<(), String> {
        let on = api.post_json("/render", r#"{"headless": true}"#)?;
        report.is_true("headless on", on["headless"] == serde_json::json!(true));
        report.is_true("headless took the overlay", on["skip_overlay"] == serde_json::json!(true));
        report.is_true("headless leaves present alone", on["skip_present"] == serde_json::json!(false));
        report.is_true("headless took the geometry", on["skip_draw"] == serde_json::json!(true));

        let during = api.get_json("/state")?;
        report.equal(
            "state reflects headless",
            &during["render"]["headless"],
            &serde_json::json!(true),
        );
        report.equal("cap lifted", &during["fps_cap"]["cap"], &serde_json::json!("off"));

        // The picture freezes for half a second while headless is on — put it back.
        std::thread::sleep(Duration::from_millis(500));

        let reset = api.post_json("/render", r#"{"reset": true}"#)?;
        report.is_true("reset: headless off", reset["headless"] == serde_json::json!(false));
        report.is_true("reset: overlay", reset["skip_overlay"] == serde_json::json!(false));
        report.is_true("reset: present", reset["skip_present"] == serde_json::json!(false));
        report.is_true("reset: draw", reset["skip_draw"] == serde_json::json!(false));

        let after = api.get_json("/state")?;
        report.equal("cap came back", &after["fps_cap"]["cap"], &cap_before);

        let granular = api.post_json("/render", r#"{"skip_present": true}"#)?;
        report.is_true("granular skip_present", granular["skip_present"] == serde_json::json!(true));
        report.is_true(
            "granular did not enable headless",
            granular["headless"] == serde_json::json!(false),
        );

        Ok(())
    })();

    if let Err(error) = outcome {
        report.error("/render", error);
    }

    // Either way, leave rendering on.
    let _ = api.post("/render", r#"{"reset": true}"#);

    // An empty body asks for nothing, which the API refuses.
    match api.post("/render", "{}") {
        Err(error) if status_of(&error) == Some(400) => report.pass("empty /render body -> 400"),
        Err(error) => report.error("empty /render body (expected 400)", error),
        Ok(()) => report.fail("empty /render body should be 400"),
    }
}

/// Step 12 — the load test: many `/state` requests at once.
///
/// The API is single-threaded and non-blocking-accept, so this checks that a burst of clients gets
/// served rather than dropped. The requests are spread over a few threads because the client here is
/// blocking; the point is the server's behaviour, not the client's throughput.
fn load_test(api: &Api, report: &mut Report) {
    println!();
    step(&format!("Step 12: load test ({PARALLEL_REQUESTS} parallel /state)"));

    let workers = 4;
    let per_worker = PARALLEL_REQUESTS / workers;
    let oks: usize = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| scope.spawn(|| (0..per_worker).filter(|_| api.get("/state").is_ok()).count()))
            .collect();

        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .sum()
    });

    let failed = PARALLEL_REQUESTS - oks;
    println!("  {oks}/{PARALLEL_REQUESTS} OK, {failed} failed");
    report.is_true("every parallel request answered", failed == 0);
}

/// Step 13 — `POST /eject`, which unloads the DLL and is expected to release the port.
fn eject_check(api: &Api, report: &mut Report) {
    println!();
    step("Step 13: POST /eject");

    match api.post_json("/eject", "") {
        Ok(value) => report.equal("ejecting response", &value["ejecting"], &serde_json::json!(true)),
        Err(error) => return report.error("/eject", error),
    }

    // The render loop handles the flag on its next frame, and the port frees after the HTTP thread's
    // shutdown, whose joins are bounded by socket timeouts.
    std::thread::sleep(Duration::from_secs(2));

    match api.get("/health") {
        Err(_) => report.pass("API unreachable after eject (port released)"),
        Ok(()) => report.fail("API still alive after eject"),
    }
}

/// The one command both script bodies use: walk forward for a third of a second.
fn walk_command() -> ScriptCommand {
    ScriptCommand {
        t: 0,
        duration: 20,
        input: ScriptInput {
            forward: true,
            ..ScriptInput::default()
        },
        when_enemy: None,
    }
}

/// A request body from the mod's own types, so a format change is a compile error here.
///
/// A serialisation failure is impossible for these types, so it is a report entry rather than an
/// aborted run: it cannot be the thing that stops the other checks from running.
fn body(request: &ScriptRequest, report: &mut Report) -> Option<String> {
    match serde_json::to_string(request) {
        Ok(json) => Some(json),
        Err(error) => {
            report.error("serialising the request body", error);
            None
        }
    }
}
