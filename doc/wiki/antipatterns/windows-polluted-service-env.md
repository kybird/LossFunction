---
status: active
version_context: "Windows 10/11 + Git Bash + conda 서비스 운영 (2026-09 실측)"
tags: [windows, environment, anti-pattern]
aliases: [dll-search-order, polluted-path-service-start, msys-path-conversion]
created: 2026-09-14
confidence: 4
---

# 오염된 환경의 서비스 기동 (Windows)

Windows에서 외부 프로세스(서비스/빌드)를 현재 셸 환경 그대로 기동하면,
PATH의 타 벤더 DLL이나 MSYS 경로 변환이 치명적 실패을 만든다. **서버류는
최소 환경으로, 바이너리는 절대경로로.**

## 실패 양상 (실측, 전부 당한 사례)

1. **백엔드 DLL 크래시**: 개발 PATH를 물려받은 PostgreSQL 백엔드가
   타 벤더 DLL(libssl/icu 등)과 충돌해 사망:
   ```
   LOG:  server process (PID 45596) was terminated by exception 0xC0000142
   LOG:  could not reserve shared memory region (addr=...) for child ...: error code 487
   ```
   클라이언트 증상: `ConnectionResetError: [WinError 64] ...`. psql 단발은
   우연히 통과해 원인 특정이 늦었다. 해결: **최소 PATH(자체 bin +
   System32)와 필수 환경변수만으로 기동** — 이후 15개 동시 연결 스트레스
   크래시 0.

2. **MSYS 경로 변환**: Git Bash에서 원격 명령의 단일 `/path` 토큰은
   `C:/Program Files/Git/path...`로 왜곡 — `~/` 상대경로로만 전달.

3. **PATH 의존 바이너리**: Git Bash PATH의 GNU tar는 `C:\...` 경로를
   host:path로 오인:
   ```
   tar (child): Cannot connect to C: resolve failed
   ```
   → Windows bsdtar를 절대경로로 지정. ssh도 마찬가지(Git Bash ssh는
   Windows ssh-agent 네임드파이프를 못 읽음 → `ssh.exe` 절대경로).

## 예방 체크리스트

- [ ] 서버/데몬은 `env -i` 또는 최소 환경으로 기동했는가
- [ ] tar/ssh 등 시스템 바이너리는 절대경로인가
- [ ] 원격 명령의 경로는 `~/` 상대경로인가
- [ ] "가끔 된다"는 증상은 주소/환경 복불복이라 의심부터 하는가

## Related

[[deployment-operations]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 7 (PostgreSQL DLL 크래시, `hash:afb6004`)
- doc/raw/2026-09-14.md Case 22 (GNU tar 함정, `hash:4f716c5`)
