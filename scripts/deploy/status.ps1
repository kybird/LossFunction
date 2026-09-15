<#
.SYNOPSIS
    배포된 LossFunction 상태(컨테이너 + 헬스 + 데이터 파일)를 확인한다.
#>
param(
    [string]$HostName = "vault",
    [string]$App = "lossfunction"
)

$ssh = "C:\Windows\System32\OpenSSH\ssh.exe"
$remote = "~/$App"

Write-Host "== containers =="
& $ssh -o BatchMode=yes $HostName "cd $remote && docker compose ps"

Write-Host ""
Write-Host "== health =="
& $ssh -o BatchMode=yes $HostName "curl -s --max-time 5 http://127.0.0.1:18080/healthz; echo"

Write-Host ""
Write-Host "== data volume =="
& $ssh -o BatchMode=yes $HostName "cd $remote && docker compose exec -T runtime ls -la /data 2>/dev/null || echo '(no db file yet)'"
