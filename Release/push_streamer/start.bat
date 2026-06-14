@echo off
chcp 65001 >nul
title push_streamer

echo ==============================================
echo   EcoAlert push_streamer - HLS Test Streamer
echo ==============================================
echo.

:: Check Python
python --version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Python not found. Please install Python 3.10+ and add to PATH.
    echo         https://www.python.org/downloads/
    pause
    exit /b 1
)

:: Check ffmpeg
ffmpeg -version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] ffmpeg not found. Please install and add to PATH.
    echo         Install: winget install ffmpeg
    echo         Download: https://ffmpeg.org/download.html
    pause
    exit /b 1
)

:: Install dependencies if needed
python -c "import yaml, aiohttp" >nul 2>&1
if errorlevel 1 (
    echo [INFO] Installing Python dependencies...
    pip install -r requirements.txt
    if errorlevel 1 (
        echo [ERROR] Failed to install dependencies.
        pause
        exit /b 1
    )
    echo.
)

:: Check Video directory
if not exist "..\..\Video" (
    echo [WARNING] Video directory not found at ..\..\Video
    echo           Please place test video MP4 files in the Video directory
    echo           next to the Release folder.
    echo.
)

:: Check if port 8080 is already in use
netstat -ano | findstr ":8080" | findstr "LISTENING" >nul 2>&1
if not errorlevel 1 (
    echo [WARNING] Port 8080 is already in use.
    for /f "tokens=5" %%p in ('netstat -ano ^| findstr ":8080" ^| findstr "LISTENING"') do (
        echo           PID: %%p
        echo           Killing old process...
        taskkill /F /PID %%p >nul 2>&1
    )
    timeout /t 2 /nobreak >nul
    echo.
)

echo Starting push_streamer (config mode, 8 HLS streams)...
echo HLS endpoints: http://127.0.0.1:8080/cam-{1~8}/index.m3u8
echo Press Ctrl+C to stop.
echo.

python -m push_streamer.cli --config config.example.yaml

pause
