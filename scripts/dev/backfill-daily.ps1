<#
.SYNOPSIS
    금고(Vaultwarden)에서 KIS 자격증명을 코드가 직접 읽어 일봉 백필을 실행한다.

.DESCRIPTION
    에이전트/수동 복사 없이: 이 스크립트가 bw CLI로 금고를 열고 항목을 조회해
    환경변수로만 Rust 백필 CLI(--backfill-daily)에 전달한다.

    - 마스터 비밀번호는 마스킹 프롬프트로 입력받는다(화면/로그 미출력).
      ~/.bw_session에 이미 유효한 세션이 있으면 프롬프트 없이 재사용.
    - 키 값은 어디에도 출력·저장되지 않고 이 프로세스 트리에만 존재한다.
    - 백필은 조회 API(FHKST03010100)만 사용 — 이 경로로 주문이 나갈 수 없다.
    - 백필 자체는 읽기 전용이라 실전 도메인(real) 사용이 안전하며, 기본값이다.
      주문 경로의 paper+real 가드는 이 스크립트와 무관하게 계속 유효하다.

    항목 구조(금고 "KIS 실전투자"): username=AppKey, password=AppSecret,
    notes=계좌번호. 모의 키를 확보하면 -ItemName "KIS 모의투자" -Environment mock.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\dev\backfill-daily.ps1
    powershell -ExecutionPolicy Bypass -File scripts\dev\backfill-daily.ps1 -Years 10
    powershell -ExecutionPolicy Bypass -File scripts\dev\backfill-daily.ps1 -ItemName "KIS 모의투자" -Environment mock
#>
param(
    # 금고에서 읽을 항목 이름
    [string]$ItemName = "KIS 실전투자",

    # 백필 기간(년, 기본 5)
    [int]$Years = 5,

    # KIS 도메인: real(기본 — 읽기 전용이라 안전) | mock
    [ValidateSet("real", "mock")]
    [string]$Environment = "real",

    # 종목 유니버스(쉼표 구분). 미지정 시 앱 기본 watchlist 사용.
    [string]$Watchlist = ""
)

$ErrorActionPreference = "Stop"

# bw is an npm wrapper that resolves `node` from PATH — conda-activated or
# stale sessions can shadow it. Pin the standard install location if missing.
if (-not (Get-Command node -ErrorAction SilentlyContinue) -and
    (Test-Path "C:\Program Files
odejs
ode.exe")) {
    $env:Path = "C:\Program Files
odejs;$env:Path"
}



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
    Set-Content -Path "$HOME\.bw_session" -Value $session -NoNewline
    $env:BW_SESSION = $session
}

# ── 1. 금고 세션 확보 ─────────────────────────────────────────────────────
if (Test-Path "$HOME\.bw_session") {
    $env:BW_SESSION = (Get-Content "$HOME\.bw_session" -Raw)
}
$status = Get-BwStatus
if ($status -eq "unauthenticated") {
    Write-Host "bw가 로그인되어 있지 않다. 최초 1회:" -ForegroundColor Yellow
    Write-Host "  bw config server https://vault.kybird.dynu.net; bw login"
    exit 1
}
if ($status -ne "unlocked") {
    if ([Console]::IsInputRedirected) {
        Write-Error "잠긴 금고 + 비대화형 실행 — 사용자 터미널에서 실행할 것."
    }
    Unlock-Vault
    if ((Get-BwStatus) -ne "unlocked") { Write-Error "unlock 후에도 잠겨 있음." }
}
bw sync | Out-Null

# ── 2. 금고 → 환경변수 (이 프로세스 트리에만) ─────────────────────────────
function Get-BwValue([string]$Getter, [string]$Item) {
    $val = switch ($Getter) {
        "username" { bw get username $Item 2>$null }
        "password" { bw get password $Item 2>$null }
        "notes"    { bw get notes $Item 2>$null }
    }
    if (-not $val) { Write-Error "금고 항목 '$Item'($Getter)에서 값을 찾지 못함 — 항목 구조를 확인하세요." }
    $val
}

Write-Host "[vault] 항목 '$ItemName'에서 자격증명 조회 중..."
$env:KIS_APP_KEY = Get-BwValue "username" $ItemName
$env:KIS_APP_SECRET = Get-BwValue "password" $ItemName
$env:KIS_ACCOUNT_NUMBER = (Get-BwValue "notes" $ItemName).Trim()
$env:KIS_ENVIRONMENT = $Environment
# 주문 경로는 paper+memory로 고정 — 백필은 조회만 하므로 real 도메인을 쓰더라도
# 이 프로세스가 주문을 만들 수 없다(가드와 무관한 이중 구조).
$env:TRADING_MODE = "paper"
$env:PAPER_BACKEND = "memory"
if ($Watchlist) { $env:WATCHLIST = $Watchlist }
# cargo는 rust/에서 실행되므로 상대경로 DB는 엉뚱한 곳에 생긴다 — 저장소 루트로 고정.
$rootEarly = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$dbPath = Join-Path $rootEarly "data\lossfunction.db"
New-Item -ItemType Directory -Force -Path (Split-Path $dbPath) | Out-Null
$env:DATABASE_PATH = $dbPath

Write-Host "[run] 백필 ${Years}년 — 도메인 $Environment, 저장 $dbPath (candles)"

# ── 3. 백필 실행 ───────────────────────────────────────────────────────────
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Push-Location (Join-Path $root "rust")
try {
    cargo run --release -- --backfill-daily $Years
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}
