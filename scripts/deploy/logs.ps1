<#
.SYNOPSIS
    배포된 LossFunction 로그를 팔로우한다 (Ctrl+C로 종료).
#>
param(
    [string]$HostName = "vault",
    [string]$App = "lossfunction",
    [int]$Tail = 100
)

$ssh = "C:\Windows\System32\OpenSSH\ssh.exe"
& $ssh -o BatchMode=yes $HostName "cd ~/$App && docker compose logs -f --tail $Tail runtime"
