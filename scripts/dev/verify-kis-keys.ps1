<#
.SYNOPSIS
    KIS AppKey/AppSecret 쌍이 유효한지 확인한다 (토큰 발급 시도).

.DESCRIPTION
    가장 가벼운 검증 경로: POST /oauth2/tokenP 로 액세스 토큰 발급을 시도한다.
    - 모의 도메인(openapivts) → 성공하면 "모의투자용 키"
    - 실전 도메인(openapi)    → 성공하면 "실전용 키"
    - 둘 다 실패하면 유효하지 않은 키 (또는 네트워크/권한 문제 — 메시지 참고)

    키/시크릿은 화면에 출력되지 않는다 (마스킹 입력). 발급된 토큰도 앞 8자만
    표시한다. 이 스크립트는 아무것도 저장하지 않는다.

    주의: tokenP 성공은 "키 자체가 유효 + 해당 도메인용"임을 뜻한다.
    조회/주문 권한(서비스 신청 범위)은 별개다 — 그건 첫 실증에서 확인.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\dev\verify-kis-keys.ps1
#>
param(
    # 확인할 도메인: auto(기본 — 모의→실전 순서로 시도), mock, real
    [ValidateSet("auto", "mock", "real")]
    [string]$Environment = "auto"
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Read-Masked([string]$Prompt) {
    $sec = Read-Host -Prompt $Prompt -AsSecureString
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
    $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    $plain
}

function Invoke-TokenIssue([string]$BaseUrl, [string]$AppKey, [string]$AppSecret) {
    $body = @{
        grant_type = "client_credentials"
        appkey     = $AppKey
        appsecret  = $AppSecret
    } | ConvertTo-Json
    try {
        $response = Invoke-RestMethod -Method Post -Uri "$BaseUrl/oauth2/tokenP" `
            -ContentType "application/json" -Body $body -TimeoutSec 20
        return @{ Ok = $true; Response = $response }
    }
    catch {
        $detail = ""
        if ($_.ErrorDetails -and $_.ErrorDetails.Message) { $detail = $_.ErrorDetails.Message }
        elseif ($_.Exception.Response) {
            $detail = "HTTP $([int]$_.Exception.Response.StatusCode)"
        }
        return @{ Ok = $false; Detail = $detail }
    }
}

Write-Host "== KIS 키 검증 (tokenP 발급 시도) ==" -ForegroundColor Cyan
$appKey = Read-Masked "AppKey"
if (-not $appKey) { Write-Error "AppKey가 비었다."; exit 1 }
$appSecret = Read-Masked "AppSecret"
if (-not $appSecret) { Write-Error "AppSecret이 비었다."; exit 1 }

$mockUrl = "https://openapivts.koreainvestment.com:9443"
$realUrl = "https://openapi.koreainvestment.com:9443"

$targets = switch ($Environment) {
    "mock" { @(@{ Name = "모의투자"; Url = $mockUrl }) }
    "real" { @(@{ Name = "실전"; Url = $realUrl }) }
    default { @(@{ Name = "모의투자"; Url = $mockUrl }, @{ Name = "실전"; Url = $realUrl }) }
}

$anySuccess = $false
foreach ($target in $targets) {
    Write-Host ""
    Write-Host "[$($target.Name)] $($target.Url) 에 시도 중..."
    $result = Invoke-TokenIssue $target.Url $appKey $appSecret

    if ($result.Ok -and $result.Response.access_token) {
        $anySuccess = $true
        $tokenPrefix = $result.Response.access_token.Substring(0, [Math]::Min(8, $result.Response.access_token.Length))
        Write-Host "  -> 성공: 토큰 발급됨 ($tokenPrefix…)" -ForegroundColor Green
        if ($result.Response.access_token_token_expired) {
            Write-Host "  -> 만료 예정(FA): $($result.Response.access_token_token_expired) (KST)"
        }
        Write-Host "  ==> 이 키는 [$($target.Name)] 용입니다." -ForegroundColor Green
        break
    }

    Write-Host "  -> 실패" -ForegroundColor Yellow
    if ($result.Detail) { Write-Host "     상세: $($result.Detail)" }
}

Write-Host ""
if ($anySuccess) {
    Write-Host "판정: 유효한 KIS 키." -ForegroundColor Green
    Write-Host "다음 단계: 금고(Vaultwarden) 항목 'KIS 모의투자'에 username=AppKey, password=AppSecret, notes=계좌번호 로 저장."
}
else {
    Write-Host "판정: 두 도메인 모두 실패 — 키가 유효하지 않거나 만료/권한 문제." -ForegroundColor Red
    Write-Host "참고: 미국 등 해외 IP 차단일 수도 있다(상세 메시지 확인). 서류/발급 상태는 KIS 포털 앱 관리 화면에서."
}
