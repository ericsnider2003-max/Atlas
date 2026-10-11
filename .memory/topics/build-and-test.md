# build-and-test

- Phone builds use --features phone-llm (phone-llm-metal on iPhone/iPad); the desktop build is unchanged without them (source: NEW_CHAT_START_HERE.md, 2026-09-27). _(2026-10-11)_
- Tests needing outside files: ATLAS_TOR_BUNDLE=<expert bundle folder> cargo test --test all tor_ships -- --ignored; ATLAS_PHONE_TEST_MODEL=<gguf> cargo test --features phone-llm --test the_phone_thinks_for_itself -- --nocapture (source: NEW_CHAT_START_HERE.md, 2026-09-27). _(2026-10-11)_
