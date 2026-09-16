@echo off
setlocal
title rqbit Docker image build

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-docker-image.ps1" %*
set "BUILD_EXIT_CODE=%ERRORLEVEL%"

if not "%BUILD_EXIT_CODE%"=="0" (
    echo.
    echo Build failed with exit code %BUILD_EXIT_CODE%.
) else (
    echo.
    echo Build completed successfully.
)

echo.
pause
exit /b %BUILD_EXIT_CODE%
