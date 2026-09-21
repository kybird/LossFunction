<#
.SYNOPSIS
    개발 머신에서 LossFunction을 Vaultwarden 시크릿 주입과 함께 실행한다.

.DESCRIPTION
    bw(Bitwarden CLI) 상태 확인 -> 잠겨 있으면 마스터 비밀번호를 입력받아(마스킹)
    unlock -> 금고에서 KIS 자격증명을 조회해 이 프로세스 트리에만 환경변수로 주입 ->
    rust/에서 cargo run --release.

    - 마스터 비밀번호는 명령줄이 아닌 임시 환경변수(--passwordenv)로 전달하고
      사용 즉시 지운다(프로세스 목록 노출 방지).
    - 시크릿 값은 절대 화면 출력/저장소 기록하지 않는다. 주입 결과는 변수명만 표시.
    - unlock 세션은 ~/.bw_session에 갱신한다(에이전트 세션과 공유, bw lock 시 만료).
    - 금고에서 찾지 못한 자격증명은 경고 후 계속한다(paper+memory는 불필요).

    최초 1회는 이 머신에서 (대화형):
        bw config server https://vault.kybird.dynu.net
        bw login

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\dev\run-local.ps1
    powershell -ExecutionPolicy Bypass -File scripts\dev\run-local.ps1 -Backend kis
    powershell -ExecutionPolicy Bypass -File scripts\dev\run-local.ps1 -TradingMode paper -Backend memory -CargoArgs -- --bin lossfunction
#>
param(
    [ValidateSet("paper", "live")]
    [string]$TradingMode = "paper",

    # Paper 브로커: memory(기본 가상) | kis(모의투자 도메인 — 금고 자격증명 필요)
    [ValidateSet("memory", "kis")]
    [string]$Backend = "memory",

    # KIS 자격증명을 읽어올 금고 항목. 기본은 모의투자용; 실전 키로 상태 페이지의
    # 일봉 갱신 버튼(읽기 전용)을 쓰려면 -KisItem "KIS 실전투자".
    [string]$KisItem = "KIS 모의투자",

    [bool]$DemoLoop = $true,

    # 실데이터 리플레이 시뮬레이션: 저장된 실제 일봉으로 매매 시뮬레이션(주문 없음).
    # 별도 시뮬 DB를 사용해 랜덤 데모 데이터와 분리한다.
    [switch]$Simulate,

    # 시뮬레이션에 쓸 전략 키 (기본 sma-cross — 전체 목록은 /lab 참고)
    [string]$SimStrategy = "sma-cross",

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

# bw is an npm wrapper that resolves `node` from PATH — conda-activated or
# stale sessions can shadow it. Pin the standard install location if missing.
if (-not (Get-Command node -ErrorAction SilentlyContinue) -and
    (Test-Path "C:\Program Files\nodejs\node.exe")) {
    $env:Path = "C:\Program Files\nodejs;" + $env:Path
}



# ── 금고 항목 매핑 — 실제 Vaultwarden 항목 구조에 맞으면 이곳만 수정 ──────
# KIS 자격증명을 하나의 로그인 항목으로 가정: username=AppKey, password=AppSecret,
# notes=계좌번호. 항목을 분리해 두었다면 아래 3개의 bw get 호출을 각자 맞춰 고칠 것.
# (GLM_API_KEY는 런타임이 아직 읽지 않음 — GLM 자동 호출 카드에서 추가 예정)

function Get-BwStatus {
    try { (bw status | ConvertFrom-Json).status } catch { "unknown" }
}

function Unlock-Vault {
    $sec = Read-Host -Prompt "Vaultwarden 마스터 비밀번호" -AsSecureString
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
    $env:BW_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    try {
        $session = bw unlock --passwordenv BW_PASSWORD --raw
    } finally {
        Remove-Item Env:BW_PASSWORD -ErrorAction SilentlyContinue
    }
    if (-not $session) { Write-Error "unlock 실패 — 비밀번호를 확인하세요." }
    # 세션 공유: 에이전트(vaultwarden-secrets 스킬)와 같은 파일을 쓴다.
    Set-Content -Path "$HOME\.bw_session" -Value $session -NoNewline
    $env:BW_SESSION = $session
}

# ── 1. 세션 확보 ─────────────────────────────────────────────────────────
if (Test-Path "$HOME\.bw_session") {
    $env:BW_SESSION = (Get-Content "$HOME\.bw_session" -Raw)
}

$status = Get-BwStatus
if ($status -eq "unauthenticated") {
    Write-Host "bw가 이 서버에 로그인되어 있지 않다. 최초 1회 대화형 세팅:" -ForegroundColor Yellow
    Write-Host "  bw config server https://vault.kybird.dynu.net"
    Write-Host "  bw login"
    exit 1
}
if ($status -ne "unlocked") {
    # stdin이 리다이렉트된(에이전트/CI) 실행은 프롬프트에 걸려 멈춘다 — 즉시 실패.
    if ([Console]::IsInputRedirected) {
        Write-Error "잠긴 금고 + 비대화형 실행 — 비밀번호 입력 불가. 사용자 터미널에서 실행하거나 ~/.bw_session을 먼저 준비하세요."
    }
    Unlock-Vault
    if ((Get-BwStatus) -ne "unlocked") { Write-Error "unlock 후에도 잠겨 있음 — 세션 확인 필요." }
}
bw sync | Out-Null

# ── 2. 금고 → 환경변수 (이 프로세스 트리에만 존재) ────────────────────────
function Set-SecretFromVault {
    param([string]$VarName, [string]$Getter, [string]$Item)
    # bw get matches item IDs, not display names — resolve name -> id first.
    $raw = (bw list items 2>$null | Out-String)
    $id = ($raw | ConvertFrom-Json) |
        Where-Object { $_.name -eq $Item } |
        Select-Object -First 1 -ExpandProperty id
    if (-not $id) { Write-Host "  $VarName : 금고 항목 '$Item' 없음 — 미주입" -ForegroundColor Yellow; return }
    $val = switch ($Getter) {
        "username" { bw get username $id 2>$null }
        "password" { bw get password $id 2>$null }
        "notes"    { bw get notes $id 2>$null }
    }
    if ($val) {
        Set-Item -Path ("Env:" + $VarName) -Value $val
        Write-Host "  $VarName : 주입 완료"
    } else {
        Write-Host "  $VarName : 금고 항목 '$Item'($Getter)에서 찾지 못함 — 미주입" -ForegroundColor Yellow
    }
}

# GLM API key (optional): 자연어 전략 생성 기능용. 항목이 없으면 경고만.
$glmKey = bw get password "GLM API key" 2>$null
if ($glmKey) {
    $env:GLM_API_KEY = $glmKey
    Write-Host "  GLM_API_KEY : 주입 완료"
} else {
    Write-Host "  GLM_API_KEY : 금고 항목 없음 — 자연어 전략 생성 비활성" -ForegroundColor Yellow
}

Write-Host "[vault] KIS 자격증명 조회 (항목: $KisItem)"
Set-SecretFromVault VarName "KIS_APP_KEY"      Getter "username" Item $KisItem
Set-SecretFromVault VarName "KIS_APP_SECRET"   Getter "password" Item $KisItem
Set-SecretFromVault VarName "KIS_ACCOUNT_NUMBER" Getter "notes"  Item $KisItem

# ── 3. 실행 ───────────────────────────────────────────────────────────────
$env:TRADING_MODE = $TradingMode
$env:PAPER_BACKEND = $Backend
if ($Mode -eq "sim" -or $Simulate) {
    $env:SIMULATION = "true"
    if (-not $SimStrategy) {
        Write-Host "  Strategies: sma-cross | rsi-reversion | donchian-breakout | bollinger-reversion | momentum-rotation | macd-cross"
        $picked = Read-Host "  Strategy (default sma-cross)"
        if ($picked) { $SimStrategy = $picked } else { $SimStrategy = "sma-cross" }
    }
    $env:SIM_STRATEGY = $SimStrategy
    $env:DEMO_LOOP = "false"
    $env:DATABASE_PATH = Join-Path $root "data\sim.db"
    $env:SIM_SOURCE = Join-Path $root "data\lossfunction.db"
    Write-Host "[sim] 실데이터 리플레이 시뮬레이션 (전략 $SimStrategy, DB data/sim.db)"
} else {
    $env:SIMULATION = "false"
    $env:DEMO_LOOP = if ($DemoLoop) { "true" } else { "false" }
}
# cargo는 rust/에서 실행되므로 DB 경로를 저장소 루트 기준으로 고정한다.

Write-Host "[run] TRADING_MODE=$TradingMode PAPER_BACKEND=$Backend DEMO_LOOP=$($env:DEMO_LOOP)"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Push-Location (Join-Path $root "rust")
try {
    if ($CargoArgs) { cargo run --release @CargoArgs }
    else { cargo run --release }
} finally {
    Pop-Location
}
