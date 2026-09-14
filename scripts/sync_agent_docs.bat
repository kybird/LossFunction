@echo off
REM llm-wiki-template-version: 1 — 이 줄이 살아있으면 llm-wiki init이 갱신하고, 지워져 있으면 사용자 수정본으로 건너뛴다 (plan.md 6.1).
REM CLAUDE.md(정본) -> 복사본 동기화.
REM 복사본 목록에 새 에이전트 파일을 추가하면 자동으로 동기화된다.
REM 항상 CLAUDE.md만 편집하고 이 스크립트를 실행할 것.
setlocal
cd /d "%~dp0.."

set "CANON=CLAUDE.md"
REM 복사본 목록 — 새 에이전트 파일은 여기에 추가.
call :sync_one agents.md
if errorlevel 1 exit /b 1
call :sync_one GEMINI.md
if errorlevel 1 exit /b 1

echo [sync] 모든 복사본 동기화 OK.
endlocal
exit /b 0

REM --- 서브루틴: sync_one <mirror> ---
:sync_one
copy /y "%CANON%" "%~1" >nul
if errorlevel 1 (
    echo [sync] ERROR: %~1 복사 실패
    exit /b 1
)
fc "%CANON%" "%~1" >nul
if errorlevel 1 (
    echo [sync] ERROR: %~1 동기화 후에도 불일치
    exit /b 1
)
echo [sync] %CANON% -^> %~1 OK
exit /b 0
