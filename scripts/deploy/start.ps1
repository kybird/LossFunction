<#
.SYNOPSIS
    중지된 LossFunction을 재기동한다 (빌드 없음 — 코드 갱신은 deploy.ps1).
#>
param(
    [string]$HostName = "vault",
    [string]$App = "lossfunction"
)

$ssh = "C:\Windows\System32\OpenSSH\ssh.exe"
& $ssh -o BatchMode=yes $HostName "cd ~/$App && docker compose up -d"
& $ssh -o BatchMode=yes $HostName "sleep 5 && curl -s --max-time 3 http://127.0.0.1:18080/healthz; echo"
