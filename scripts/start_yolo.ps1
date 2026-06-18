param(
    [string]$Python = "python",
    [int]$StartupTimeoutSec = 60
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$server = Join-Path $root "Algo\YOLO\server_ws.py"
$requirements = Join-Path $root "Algo\YOLO\requirements.txt"

if (-not (Test-Path -LiteralPath $server)) {
    throw "YOLO server not found: $server"
}

& $Python -c "import fastapi, uvicorn, cv2, numpy, yaml, ultralytics, websockets"
if ($LASTEXITCODE -ne 0) {
    throw "YOLO dependencies are missing. Run: $Python -m pip install -r `"$requirements`""
}

$process = Start-Process -FilePath $Python -ArgumentList $server -WorkingDirectory $root -WindowStyle Hidden -PassThru
$deadline = (Get-Date).AddSeconds($StartupTimeoutSec)
do {
    if ($process.HasExited) {
        throw "YOLO server exited during startup (exit code $($process.ExitCode))"
    }
    try {
        $health = Invoke-RestMethod -Uri "http://127.0.0.1:8090/health" -TimeoutSec 2
        if ($health.status -eq "healthy") {
            Write-Output "YOLO server ready (PID=$($process.Id), device=$($health.device))"
            exit 0
        }
    } catch {
        Start-Sleep -Milliseconds 500
    }
} while ((Get-Date) -lt $deadline)

Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
throw "YOLO server did not become healthy within $StartupTimeoutSec seconds"
