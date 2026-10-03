# Social fixtures: where each shape comes from

Two kinds of file live here (29 Sep 2026).

## Real answers, saved from the live services

These were fetched from the build machine on 29 Sep 2026 and trimmed to a few items. The shape is left untouched.

| File | Fetched from |
|---|---|
| `youtube_channel.xml` | `https://www.youtube.com/feeds/videos.xml?channel_id=UCBJycsmduvYEL83R_U4JriQ` |
| `bluesky_profile.json` | `https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile?actor=bsky.app` |
| `bluesky_author_feed.json` | `.../app.bsky.feed.getAuthorFeed?actor=bsky.app&limit=3` |
| `mastodon_tag.json` | `https://fosstodon.org/api/v1/timelines/tag/rustlang?limit=3` (mastodon.social answered 503) |
| `hn_front_page.json` | `https://hn.algolia.com/api/v1/search?tags=front_page&hitsPerPage=3` |
| `producthunt.xml` | `https://www.producthunt.com/feed` |
| `google_trends.xml` | `https://trends.google.com/trending/rss?geo=US` |

Reddit's `/r/rust/new/.rss` answered 429 from this machine on both tries, so there is no saved Reddit sample. `social::watchlist` reads Reddit with the same feed reader (`feeds::parse`) as Product Hunt's Atom.

## Synthetic exports that copy the published formats

None of these came from a real account. Each copies the layout the platform documents or that is widely described. Where that layout could not be confirmed in 2026, it is marked unverified in `src/social/exports.rs`.

- `exports/x_archive/` and `exports/x_archive.zip` copy an X archive ("Download an archive of your data"). `data/tweets.js` is a `window.YTD.tweets.part0 = [...]` array of `{"tweet": {...}}` records with string counts and `created_at` written as "Tue Sep 24 14:03:11 +0000 2024". The fixture also has `account.js`, `follower.js`, and `manifest.js` with `generationDate`. One post is a repost ("RT @"), which the importer must leave out.
- `exports/tiktok/user_data_tiktok.json` copies TikTok's "Download your data" JSON: `Profile > Profile Information > ProfileMap`, `Activity > Follower List > FansList`, and `Video > Videos > VideoList` holding `Date`, `Link` and `Likes`. It has no views. Whether real exports carry views is unverified.
- `exports/instagram/` copies Instagram's "Download your information" JSON: `your_instagram_activity/content/posts_1.json` and `reels.json`, `connections/followers_and_following/followers_1.json`, and `logged_information/past_instagram_insights/{posts,reels}.json` with `string_map_data` figures. Text is stored the way Meta writes it, UTF-8 read as Latin-1 ("Hereâ\u0080\u0099s").
- `exports/Content_2026-09-01_2026-09-28_JordanLee.xlsx` copies LinkedIn's Creator analytics export (LinkedIn Help a704175). It was made with openpyxl and has these sheets:
  - `DISCOVERY`.
  - `ENGAGEMENT`, where the dates are real Excel dates.
  - `TOP POSTS`, which holds two tables side by side with string dates.
  - `FOLLOWERS`, which holds "Total followers on 9/28/2026:" followed by daily new followers.
  - `DEMOGRAPHICS`.
- `exports/youtube_studio/Table data.csv` copies YouTube Studio's Analytics "Export current view" table. It starts with a `Total` row, gives `Duration` in seconds and "Average view duration" as h:mm:ss, and puts a quoted title with doubled quotes inside it.
- `handmade.xlsx` is a minimal SpreadsheetML package written by hand. It covers what openpyxl does not write: a rich-text shared string, an inline string, a boolean, a formula's string result, a skipped row, a cell in column AB, and relationships listed out of order.

Names in the synthetic files are neutral: Jordan, Maya, Sam, Northwind.
