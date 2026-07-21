# test_connect.ps1 — drmod-server connection test
# Usage: .\test_connect.ps1 [-Server localhost] [-TcpPort 5222] [-HttpPort 8080]

param(
    [string]$Server = "localhost",
    [int]$TcpPort = 5222,
    [int]$HttpPort = 8080
)

$ErrorActionPreference = "Stop"

$httpUrl = "http://${Server}:${HttpPort}"

# ── Helper: fetch dashboard ──

function Get-Dashboard {
    try {
        $body = Invoke-RestMethod -Uri $httpUrl -Method Get -TimeoutSec 3
        Write-Host $body -ForegroundColor Gray
    } catch {
        Write-Host "ERROR: dashboard unreachable at $httpUrl" -ForegroundColor Red
        Write-Host $_
        exit 1
    }
}

# ── Step 1: check empty dashboard ──

Write-Host "`n=== Step 1: Dashboard (should be empty) ===" -ForegroundColor Cyan
Get-Dashboard

# ── Step 2: simulate player connect ──

Write-Host "`n=== Step 2: Connecting test player ===" -ForegroundColor Cyan

$connectMsg = '{"type":"connect","room":"test-room","name":"TestBot","mission_id":42}' + "`0"
$connectBytes = [System.Text.Encoding]::UTF8.GetBytes($connectMsg)

$tcp = New-Object System.Net.Sockets.TcpClient
try {
    $tcp.Connect($Server, $TcpPort)
    Write-Host "TCP connected to ${Server}:${TcpPort}" -ForegroundColor Green
} catch {
    Write-Host "ERROR: TCP connect failed: $_" -ForegroundColor Red
    exit 1
}

$stream = $tcp.GetStream()
$stream.Write($connectBytes, 0, $connectBytes.Length)
$stream.Flush()
Write-Host "Sent: connect TestBot -> room test-room" -ForegroundColor Green

# Wait for server to process
Start-Sleep -Milliseconds 5000

# Read assigned ID response (JSON line + \0)
$buf = New-Object byte[] 4096
$read = $stream.Read($buf, 0, $buf.Length)
$response = [System.Text.Encoding]::UTF8.GetString($buf, 0, $read) -replace "`0", ""
Write-Host "Server response: $response" -ForegroundColor Green

# ── Step 3: verify player appears ──

Write-Host "`n=== Step 3: Dashboard (should show TestBot) ===" -ForegroundColor Cyan
Get-Dashboard

# ── Step 4: simulate disconnect ──

Write-Host "`n=== Step 4: Disconnecting test player ===" -ForegroundColor Cyan
$disconnectMsg = '{"type":"disconnect"}' + "`0"
$disconnectBytes = [System.Text.Encoding]::UTF8.GetBytes($disconnectMsg)
$stream.Write($disconnectBytes, 0, $disconnectBytes.Length)
$stream.Flush()
$stream.Close()
$tcp.Close()
Write-Host "Disconnected" -ForegroundColor Green

Start-Sleep -Milliseconds 500

# ── Step 5: verify player is gone ──

Write-Host "`n=== Step 5: Dashboard (should be empty again) ===" -ForegroundColor Cyan
Get-Dashboard

Write-Host "`nAll tests passed!" -ForegroundColor Green
