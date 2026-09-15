<#
.SYNOPSIS
    LossFunction을 피닉스 VPS(또는 임의 SSH 호스트)에 배포한다.

.DESCRIPTION
    패키징(tar) -> 업로드(scp) -> 원격 이미지 빌드 -> docker compose 기동 ->
    헬스 확인까지 한 번에 수행한다. Windows OpenSSH만 사용하고(피닉스 규칙),
    원격 경로는 ~/ 상대경로만 쓴다. SQLite 데이터 볼륨은 재배포 시 유지된다.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\deploy\deploy.ps1
    powershell -ExecutionPolicy Bypass -File scripts\deploy\deploy.ps1 -HostName vault
#>
param(
    [string]$HostName = "vault",
    [string]$App = "lossfunction"
)

$ErrorActionPreference = "Stop"

$ssh = "C:\Windows\System32\OpenSSH\ssh.exe"
$scp = "C:\Windows\System32\OpenSSH\scp.exe"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$remote = "~/$App"

function Invoke-Remote([string]$Command) {
    & $ssh -o BatchMode=yes $HostName $Command
    if ($LASTEXITCODE -ne 0) {
        throw "remote command failed (exit $LASTEXITCODE): $Command"
    }
}

Write-Host "== [1/5] packaging from $root"
$tarball = Join-Path $env:TEMP "$App-deploy.tar.gz"
if (Test-Path $tarball) { Remove-Item $tarball }
# Absolute path: Git Bash's GNU tar would misread "C:\..." as host:path.
$tarExe = "C:\Windows\System32\tar.exe"
if (-not (Test-Path $tarExe)) { $tarExe = "tar.exe" }
& $tarExe -czf $tarball -C $root docker-compose.yml rust
if ($LASTEXITCODE -ne 0) { throw "packaging failed" }
$size = "{0:N1} KB" -f ((Get-Item $tarball).Length / 1KB)
Write-Host "   $tarball ($size)"

Write-Host "== [2/5] uploading to ${HostName}:$remote"
& $scp -o BatchMode=yes $tarball "${HostName}:${App}-deploy.tar.gz"
if ($LASTEXITCODE -ne 0) { throw "upload failed" }
Remove-Item $tarball

Write-Host "== [3/5] extracting and building image on $HostName"
Invoke-Remote "mkdir -p $remote && tar xzf ~/$App-deploy.tar.gz -C $remote && rm ~/$App-deploy.tar.gz"
Invoke-Remote "cd $remote && docker compose build"

Write-Host "== [4/5] starting (SQLite volume preserved)"
Invoke-Remote "cd $remote && docker compose up -d"

Write-Host "== [5/5] waiting for health endpoint"
$healthy = $false
for ($i = 1; $i -le 15; $i++) {
    Start-Sleep -Seconds 3
    $response = & $ssh -o BatchMode=yes $HostName "curl -s --max-time 3 http://127.0.0.1:18080/healthz" 2>$null
    if ($LASTEXITCODE -eq 0 -and $response -match '"status":\s*"ok"') {
        Write-Host "   healthy after ~$($i * 3)s: $response"
        $healthy = $true
        break
    }
}
if (-not $healthy) {
    & $ssh -o BatchMode=yes $HostName "cd $remote && docker compose logs --tail 30"
    throw "health check failed — see logs above"
}

Write-Host ""
Write-Host "deployed: $HostName ($remote)" -ForegroundColor Green
Write-Host "  status : powershell -File scripts\deploy\status.ps1"
Write-Host "  logs   : powershell -File scripts\deploy\logs.ps1"
Write-Host "  stop   : powershell -File scripts\deploy\stop.ps1"
