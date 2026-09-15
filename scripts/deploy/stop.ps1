<#
.SYNOPSIS
    LossFunction을 중지한다 (컨테이너 제거, SQLite 데이터 볼륨은 유지).
#>
param(
    [string]$HostName = "vault",
    [string]$App = "lossfunction"
)

$ssh = "C:\Windows\System32\OpenSSH\ssh.exe"
& $ssh -o BatchMode=yes $HostName "cd ~/$App && docker compose down"
Write-Host "stopped. (data volume kept — redeploy with deploy.ps1 or start.ps1)"
