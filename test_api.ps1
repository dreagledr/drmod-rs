# test_api.ps1 — drmod HTTP API smoke test
# Usage: .\test_api.ps1 [-BaseUrl http://127.0.0.1:5223]
# Требует: игра запущена, мод инжектирован (API на 127.0.0.1:5223).

param(
    [string]$BaseUrl = "http://127.0.0.1:5223"
)

$ErrorActionPreference = "Stop"
$script:Failures = 0

function Assert-Equal {
    param([string]$Name, $Actual, $Expected)
    if ($Actual -eq $Expected) {
        Write-Host "  PASS: $Name" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: $Name (expected '$Expected', got '$Actual')" -ForegroundColor Red
        $script:Failures++
    }
}

function Assert-True {
    param([string]$Name, [bool]$Condition)
    if ($Condition) {
        Write-Host "  PASS: $Name" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: $Name" -ForegroundColor Red
        $script:Failures++
    }
}

# ── Step 1: /health ──
Write-Host "`n=== Step 1: GET /health ===" -ForegroundColor Cyan
try {
    $health = Invoke-RestMethod -Uri "$BaseUrl/health" -Method Get -TimeoutSec 3
    Assert-Equal "status" $health.status "ok"
    Assert-True "base_addr present" ($null -ne $health.base_addr)
} catch {
    Write-Host "  FAIL: /health unreachable: $_" -ForegroundColor Red
    Write-Host "  Убедитесь, что игра запущена и мод инжектирован." -ForegroundColor Yellow
    $script:Failures++
}

# ── Step 2: /state ──
Write-Host "`n=== Step 2: GET /state ===" -ForegroundColor Cyan
try {
    $state = Invoke-RestMethod -Uri "$BaseUrl/state" -Method Get -TimeoutSec 3
    Assert-True "player.found" ($state.player.found -eq $true)
    Assert-True "mission_name non-empty" (-not [string]::IsNullOrEmpty($state.mission_name))
} catch {
    Write-Host "  FAIL: /state unreachable: $_" -ForegroundColor Red
    $script:Failures++
}

# ── Step 3: POST /script/run ──
Write-Host "`n=== Step 3: POST /script/run ===" -ForegroundColor Cyan
$scriptBody = @{
    name     = "smoke-test"
    commands = @(
        @{ t = 0; duration = 20; input = @{ forward = $true } }
    )
} | ConvertTo-Json -Depth 5
$scriptId = $null
try {
    $run = Invoke-RestMethod -Uri "$BaseUrl/script/run" -Method Post -Body $scriptBody -ContentType "application/json" -TimeoutSec 3
    $scriptId = $run.script_id
    Assert-True "script_id returned" ($null -ne $scriptId)
    Assert-Equal "name" $run.name "smoke-test"
} catch {
    Write-Host "  FAIL: /script/run: $_" -ForegroundColor Red
    $script:Failures++
}

# ── Step 4: GET /script/{id} ──
if ($null -ne $scriptId) {
    Write-Host "`n=== Step 4: GET /script/$scriptId ===" -ForegroundColor Cyan
    try {
        $st = Invoke-RestMethod -Uri "$BaseUrl/script/$scriptId" -Method Get -TimeoutSec 3
        Assert-Equal "id" $st.id $scriptId
        Assert-True "status in (running,done,stopped)" ($st.status -in @("running", "done", "stopped"))
    } catch {
        Write-Host "  FAIL: /script/$scriptId : $_" -ForegroundColor Red
        $script:Failures++
    }
}

# ── Step 5: GET /logs?script_id= ──
if ($null -ne $scriptId) {
    Write-Host "`n=== Step 5: GET /logs?script_id=$scriptId ===" -ForegroundColor Cyan
    try {
        $logs = Invoke-RestMethod -Uri "$BaseUrl/logs?script_id=$scriptId&limit=5" -Method Get -TimeoutSec 3
        Assert-True "count >= 0" ($logs.count -ge 0)
        Assert-True "frames is array" ($logs.frames -is [array])
    } catch {
        Write-Host "  FAIL: /logs: $_" -ForegroundColor Red
        $script:Failures++
    }
}

# ── Step 6: POST /script/stop ──
Write-Host "`n=== Step 6: POST /script/stop ===" -ForegroundColor Cyan
try {
    $stop = Invoke-RestMethod -Uri "$BaseUrl/script/stop" -Method Post -TimeoutSec 3
    Assert-True "stopped response" ($null -ne $stop)
} catch {
    if ($_.Exception.Response.StatusCode.value__ -eq 404) {
        Write-Host "  PASS: /script/stop (no active script, 404)" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: /script/stop: $_" -ForegroundColor Red
        $script:Failures++
    }
}

# ── Step 7: error paths ──
Write-Host "`n=== Step 7: error paths ===" -ForegroundColor Cyan
try {
    Invoke-RestMethod -Uri "$BaseUrl/script/run" -Method Post -Body "not json" -ContentType "application/json" -TimeoutSec 3 | Out-Null
    Write-Host "  FAIL: invalid JSON should be 400" -ForegroundColor Red
    $script:Failures++
} catch {
    if ($_.Exception.Response.StatusCode.value__ -eq 400) {
        Write-Host "  PASS: invalid JSON -> 400" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: invalid JSON: $_" -ForegroundColor Red
        $script:Failures++
    }
}
try {
    Invoke-RestMethod -Uri "$BaseUrl/script/999" -Method Get -TimeoutSec 3 | Out-Null
    Write-Host "  FAIL: /script/999 should be 404" -ForegroundColor Red
    $script:Failures++
} catch {
    if ($_.Exception.Response.StatusCode.value__ -eq 404) {
        Write-Host "  PASS: /script/999 -> 404" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: /script/999: $_" -ForegroundColor Red
        $script:Failures++
    }
}
try {
    Invoke-RestMethod -Uri "$BaseUrl/nope" -Method Get -TimeoutSec 3 | Out-Null
    Write-Host "  FAIL: /nope should be 404" -ForegroundColor Red
    $script:Failures++
} catch {
    if ($_.Exception.Response.StatusCode.value__ -eq 404) {
        Write-Host "  PASS: /nope -> 404" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: /nope: $_" -ForegroundColor Red
        $script:Failures++
    }
}

# ── Step 8: load test (20 параллельных /state) ──
Write-Host "`n=== Step 8: load test (20 parallel /state) ===" -ForegroundColor Cyan
try {
    Add-Type -AssemblyName System.Net.Http
    $client = [System.Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds(5)
    $tasks = 1..20 | ForEach-Object { $client.GetAsync("$BaseUrl/state") }
    [System.Threading.Tasks.Task]::WaitAll($tasks)
    $codes = $tasks | ForEach-Object { [int]$_.Result.StatusCode }
    $client.Dispose()
    $ok = @($codes | Where-Object { $_ -eq 200 }).Count
    $fail = @($codes | Where-Object { $_ -ne 200 }).Count
    Write-Host "  $ok/20 OK, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
    if ($fail -gt 0) { $script:Failures++ }
} catch {
    Write-Host "  FAIL: load test: $_" -ForegroundColor Red
    $script:Failures++
}

# ── Summary ──
Write-Host "`n=== Summary ===" -ForegroundColor Cyan
if ($script:Failures -eq 0) {
    Write-Host "All tests passed!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "$script:Failures check(s) failed" -ForegroundColor Red
    exit 1
}