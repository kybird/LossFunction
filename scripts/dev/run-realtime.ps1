<#
.SYNOPSIS
    금고(Vaultwarden)에서 KIS 자격증명을 읽어 실시간 시세 시뮬레이션을 로컬에서 가동한다.

.DESCRIPTION
    백필 런처(backfill-daily.ps1)와 동일한 금고 패턴: 이 스크립트가 bw CLI로
    금고를 열고 "KIS 실전투자" 항목을 조회해 환경변수로만 Rust 런타임에 전달한다.

    - QUOTES_SOURCE=kis: 실시간 체결가(H0STCNT0)를 실전 도메인에서 수신(읽기 전용).
    - 체결은 전부 MockBroker의 가짜 체결 — 이 프로세스는 주문을 만들 수 없다.
    - 상태 페이지: http://127.0.0.1:8080/ (실시간 시세·모의 체결·보유 관측).
    - 한국 개장(평일 09:00-15:30 KST)에만 틱이 흐른다 — 장외에는 접속·구독만 유지된다.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\dev\run-realtime.ps1
    powershell -ExecutionPolicy Bypass -File scripts\dev\run-realtime.ps1 -Strategy rsi-reversion
#>
param(
    # 금고에서 읽을 항목 이름
    [string]$ItemName = "KIS 실전투자",

    # KIS 도메인: real(기본 — 시세 수신은 읽기 전용이라 안전) | mock
    [ValidateSet("real", "mock")]
    [string]$Environment = "real",

    # 종목 유니버스(쉼표 구분). 미지정 시 앱 기본 watchlist 사용.
    [string]$Watchlist = "",

    # 시뮬레이션 전략(레지스트리 키). 기본 sma-cross.
    [string]$Strategy = "sma-cross"
)

$ErrorActionPreference = "Stop"

# bw is an npm wrapper that resolves `node` from PATH — conda-activated or
# stale sessions can shadow it. Pin the standard install location if missing.
if (-not (Get-Command node -ErrorAction SilentlyContinue) -and
    (Test-Path "C:\Program Files\nodejs\node.exe")) {
    $env:Path = "C:\Program Files\nodejs;" + $env:Path
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
    # bw get matches item IDs, not display names — resolve name -> id first.
    $raw = (bw list items 2>$null | Out-String)
    $id = ($raw | ConvertFrom-Json) |
        Where-Object { $_.name -eq $Item } |
        Select-Object -First 1 -ExpandProperty id
    if (-not $id) { Write-Error "금고에서 항목 '$Item'을(를) 찾지 못함 — 이름을 확인하세요." }
    $val = switch ($Getter) {
        "username" { bw get username $id 2>$null }
        "password" { bw get password $id 2>$null }
        "notes"    { bw get notes $id 2>$null }
    }
    if (-not $val) { Write-Error "항목 '$Item'($Getter)에서 값을 찾지 못함 — 항목 구조를 확인하세요." }
    $val
}

Write-Host "[vault] 항목 '$ItemName'에서 자격증명 조회 중..."
$env:KIS_APP_KEY = Get-BwValue "username" $ItemName
$env:KIS_APP_SECRET = Get-BwValue "password" $ItemName
$env:KIS_ACCOUNT_NUMBER = (Get-BwValue "notes" $ItemName).Trim()
$env:KIS_ENVIRONMENT = $Environment
# 주문 경로는 paper+memory로 고정 — 이 시뮬레이션은 시세만 실데이터를 쓰고
# 체결은 전부 가짜다(실전 도메인 시세 + MockBroker의 이중 구조).
$env:TRADING_MODE = "paper"
$env:PAPER_BACKEND = "memory"
$env:QUOTES_SOURCE = "kis"
$env:REALTIME_STRATEGY = $Strategy
if ($Watchlist) { $env:WATCHLIST = $Watchlist }
# cargo는 rust/에서 실행되므로 상대경로 DB는 엉뚱한 곳에 생긴다 — 저장소 루트로 고정.
$rootEarly = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$dbPath = Join-Path $rootEarly "data\lossfunction.db"
New-Item -ItemType Directory -Force -Path (Split-Path $dbPath) | Out-Null
$env:DATABASE_PATH = $dbPath

Write-Host "[run] 실시간 시세 시뮬레이션 — 도메인 $Environment, 전략 $Strategy, DB $dbPath"
Write-Host "[run] 상태 페이지: http://127.0.0.1:8080/ — 종료는 Ctrl+C"

# ── 3. 런타임 실행 ─────────────────────────────────────────────────────────
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Push-Location (Join-Path $root "rust")
try {
    cargo run --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}
