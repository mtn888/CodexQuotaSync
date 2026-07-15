@echo off
setlocal
chcp 65001 >nul

where pwsh.exe >nul 2>nul
if errorlevel 1 (
    set "POWERSHELL_EXE=powershell.exe"
) else (
    set "POWERSHELL_EXE=pwsh.exe"
)

"%POWERSHELL_EXE%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0update-build-all.ps1" %*
set "EXIT_CODE=%ERRORLEVEL%"

echo.
if "%EXIT_CODE%"=="0" (
    echo 一键更新、打包和重启已完成。
) else (
    echo 执行失败，退出码：%EXIT_CODE%
)
echo 按任意键关闭窗口。
pause >nul
exit /b %EXIT_CODE%
