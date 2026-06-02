# Production readiness check - 2026-05-06

This is the current pre-demo production check for the Telegram task bot.

## Verdict

Status: ready for a controlled customer demo.

The code passes the full Linux/Docker test gate and the runtime container starts healthy. The main remaining demo risk is operational, not code-level: the demo environment must use the correct bot token, real API keys, and the intended HTTP host/port.

Do not present this as a fully hands-off production system until backups, monitoring, and real Telegram smoke scenarios are checked on the actual server.

## Verified today

- `cargo fmt --all` passed.
- `cargo check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- Targeted suites passed:
  - `voice_processing_tests`: 5/5.
  - `ui_keyboard_regression_tests`: 8/8.
  - `scenario_coverage_tests`: 50/50.
- Docker/Linux full test gate passed:
  - 166 unit tests.
  - all integration suites, including task status, keyboards, voice processing, registration recovery, HTTP observability, assignee resolution, notifications, RBAC, and repository tests.
- Runtime Docker image built successfully.
- `docker compose up -d telegram-task-bot` starts the service and reports healthy.
- `docker compose --profile smoke run --rm smoke-check` passed.
- Runtime endpoints on `http://localhost:8080` passed:
  - `/healthz`
  - `/healthz/deep`
  - `/metrics`
  - `/version`

Windows note: direct local `cargo test` is still not the authoritative gate on this machine because Windows application-control policy can block one generated test executable. The same suite passes inside Linux/Docker, which is the deploy-relevant environment.

## Demo checklist

Use this before showing the bot:

1. Confirm `.env` points to the demo bot token, not a throwaway token.
2. Confirm `GOOGLE_GEMINI_API_KEY` and `OPENAI_API_KEY` are real and have budget/quota.
3. Confirm `TELEGRAM_ADMIN_IDS` contains the presenter's Telegram ID.
4. Start fresh runtime:
   ```powershell
   docker compose up -d telegram-task-bot
   docker compose ps
   ```
5. Check health:
   ```powershell
   curl.exe -fsS http://localhost:8080/healthz
   curl.exe -fsS http://localhost:8080/healthz/deep
   curl.exe -fsS http://localhost:8080/version
   ```
6. In Telegram, run:
   - `/start`
   - quick task from text
   - guided task
   - voice task, then confirm before creation
   - open task card
   - change status
   - add comment
   - report blocker
   - reassign
   - manager inbox/team tasks if the demo account is manager/admin
   - `/admin` if the demo account is admin
7. For voice demo, use a short clean Russian recording:
   - 10-25 seconds
   - one assignee name
   - one concrete action
   - optional deadline

Known local port caveat: on this workstation, `127.0.0.1:8080` was occupied by a separate `node.exe` process that returned `X-Tenant-Id header is required`. The bot container answered correctly on `localhost:8080` and inside Docker. If this happens during demo, stop the other process or use `localhost`.

## Voice and Russian transcription assessment

The implementation is well structured for Russian voice-to-task:

- Telegram voice files are downloaded through Telegram `getFile`.
- Voice metadata is validated before transcription:
  - duration limit
  - file size limit
- OpenAI transcription is called with `language=ru`.
- The configured default model is `gpt-4o-mini-transcribe`.
- Empty transcription is rejected.
- Long transcripts are normalized and clipped safely.
- Voice tasks are never created immediately after transcription.
- The user must confirm the interpreted task first.
- The user can edit the transcript before creation.
- If assignee resolution is ambiguous, the bot asks the user to choose explicitly.
- The original voice source key is preserved, so duplicate voice submissions do not silently create duplicate tasks.

Quality caveat: automated tests verify the state machine, validation, idempotency, keyboard safety, and voice draft flow. They do not prove real-world Russian audio accuracy because that requires live audio samples and the real OpenAI API. For the customer demo, record one clean sample and one noisy sample before the meeting and verify both.

Recommended demo phrase:

```text
Иван, подготовь короткий отчет по продажам за апрель и отправь мне до пятницы.
```

Expected bot behavior:

- transcription appears in Russian;
- confirmation screen appears before creation;
- assignee is either resolved or clarified;
- deadline is interpreted from the text;
- task is created only after pressing confirm.

## Monthly cost estimate

Prices checked on 2026-05-06:

- OpenAI `gpt-4o-mini-transcribe`: estimated `$0.003 / minute`; official pricing also lists `$1.25` input and `$5.00` output per 1M audio/text tokens.
  Source: https://developers.openai.com/api/docs/pricing
- Gemini 2.5 Flash: `$0.30 / 1M` text/image/video input tokens, `$1.00 / 1M` audio input tokens, `$2.50 / 1M` output tokens.
  Source: https://ai.google.dev/gemini-api/docs/pricing
- DigitalOcean Droplets start from `$4/month`, billed per second with a monthly cap.
  Source: https://www.digitalocean.com/products/droplets
- Hetzner cloud prices changed on 2026-04-01; examples: CX23 `$4.99/month`, CPX11 `$6.99/month`, CPX22 `$9.49/month`, excluding VAT.
  Source: https://docs.hetzner.com/general/infrastructure-and-availability/price-adjustment/

Recommended small production setup:

| Item | Estimate |
| --- | ---: |
| VPS, 1-2 vCPU / 2 GB RAM | `$5-10/month` |
| Backups / snapshots | `$1-3/month` |
| Domain | `$1-2/month` amortized |
| Telegram Bot API | `$0/month` |
| Google Sheets API | usually `$0/month` at this scale, quota-bound |
| OpenAI transcription | about `$0.003` per voice minute |
| Gemini task generation | about `$0.002-0.004` per task for typical short prompts |

Usage scenarios:

| Scenario | Monthly usage | AI estimate | Infra estimate | Total |
| --- | --- | ---: | ---: | ---: |
| Demo / tiny team | 300 tasks, 100 voice minutes | `$1-2` | `$6-13` | `$7-15/month` |
| Small company | 1,000 tasks, 300 voice minutes | `$3-6` | `$6-13` | `$9-19/month` |
| Active team | 3,000 tasks, 1,000 voice minutes | `$11-18` | `$8-18` | `$19-36/month` |

Practical budget recommendation: reserve `$25/month` for a comfortable first production month, or `$40/month` if you want extra buffer for monitoring, backups, and heavier voice usage.

## Production gaps to close after demo

- Run at least one live Telegram smoke test against the real customer roster.
- Turn on scheduled SQLite hot backups: `SQLITE_BACKUP_DIR=/app/data/backups`.
- Inject `GIT_SHA` during Docker build so `/version` is traceable.
- Decide whether to stop the local process occupying `127.0.0.1:8080` or change the bot port for local demos.
- Keep Docker build cache warm; first release build can take about 18 minutes on this machine.
