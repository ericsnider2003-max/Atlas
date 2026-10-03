/* The one door the phone apps call through. Implemented in src/mobile.rs. */
#ifndef ATLAS_H
#define ATLAS_H
#include <stddef.h>
#include <stdint.h>
/* Start Atlas with its data under `home` (the app's private folder), serving
   the hub on 127.0.0.1 only. `port` 0 picks a free one. Never blocks for more
   than a moment. Returns 0 once the hub answers, 1 if already running, 2 if
   it is still starting (poll atlas_mobile_url or atlas_mobile_state), and a
   negative number if it couldn't start. Only one Atlas is ever started. */
int32_t atlas_mobile_start(const char *home, uint16_t port);
/* 0 not started or stopped, 1 starting, 2 running, -1 couldn't start. */
int32_t atlas_mobile_state(void);
/* Write the hub's address (token included) into buf. Length, or -1 until
   the hub is answering. */
int32_t atlas_mobile_url(char *buf, size_t len);
/* Whether the phone is on wifi or another unmetered network: 1 yes, 0 no.
   Called at start and whenever it changes; the phone's own model downloads
   by itself only while it is 1. */
void atlas_mobile_network(int32_t unmetered);
/* Ask Atlas to stop; it finishes the turn it is on. */
void atlas_mobile_stop(void);
/* Apple's on-device model as the first brain (iOS 26+, src/applebrain.rs).
   The shell passes a function that reads a request as JSON
   ({"instructions": "...", "turns": [{"role", "content"}], "max_tokens"})
   and writes {"text": "..."} into `out` (NUL-terminated, at most `out_len`
   bytes), returning 0 answered, 1 refused, 2 too long, 3 unavailable,
   4 failed. Called from Atlas's own threads, never the main one. NULL takes
   it back. Each request it can't do goes to Atlas's own model. */
typedef int32_t (*atlas_apple_fn)(const char *req, char *out, size_t out_len);
void atlas_mobile_apple_model(atlas_apple_fn f);
/* Apple's weather (iOS 16+, src/applewx.rs): reads {"lat","lon"} and writes
   {"currentWeather": {...}, "forecastDaily": {"days": [...]}} in the shape of
   WeatherKit's REST service, returning 0 when it answered. NULL takes it back. */
typedef int32_t (*atlas_weather_fn)(const char *req, char *out, size_t out_len);
void atlas_mobile_apple_weather(atlas_weather_fn f);
#endif
