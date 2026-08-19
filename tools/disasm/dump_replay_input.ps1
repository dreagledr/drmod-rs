# Чтение последней записи из replay_record_frames (SQLite через .NET)
$dbPath = "$env:LOCALAPPDATA\drmod\runs.db"
if (-not (Test-Path $dbPath)) { Write-Host "no db"; exit 1 }

Add-Type -Path "D:\pet\drmod-rs\target\i686-pc-windows-msvc\debug\deps\libsqlite3_sys*.dll" -ErrorAction SilentlyContinue

# Используем rusqlite через готовый бинарник? Нет — используем .NET Sqlite: его нет.
# Проще: прочитать BLOB напрямую через низкоуровневый доступ — нет.
# Fallback: напечатать инструкцию вместо запуска.
Write-Host "PS cannot read sqlite without assembly; use a tool that ships sqlite3.dll"
# Попробуем найти sqlite3.exe
$sqlite = Get-Command sqlite3 -ErrorAction SilentlyContinue
if ($sqlite) {
    Write-Host "sqlite3 found: $($sqlite.Source)"
} else {
    Write-Host "sqlite3 not found"
}
