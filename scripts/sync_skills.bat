@echo off
REM llm-wiki-template-version: 1 — 이 줄이 살아있으면 llm-wiki init이 갱신하고, 지워져 있으면 사용자 수정본으로 건너뛴다 (plan.md 6.1).
REM .agents/skills/ (canon) -> .claude/skills/ (mirror) directory mirror.
REM robocopy /MIR = full mirror based on canon (removes items absent from canon).
REM Always edit .agents/skills/ only, then run this script.
setlocal
cd /d "%~dp0.."

set "CANON=.agents\skills"
set "MIRROR=.claude\skills"

if not exist "%CANON%\" (
    echo [sync-skills] ERROR: canon %CANON% not found
    exit /b 1
)

REM robocopy exit codes: 0-7 = OK (copied/identical), 8+ = error.
robocopy "%CANON%" "%MIRROR%" /MIR /NFL /NDL /NJH /NJS /NC /NS >nul
if errorlevel 8 (
    echo [sync-skills] ERROR: robocopy failed ^(exit %errorlevel%^)
    exit /b 1
)

echo [sync-skills] %CANON% -^> %MIRROR% mirror OK.
endlocal
exit /b 0
