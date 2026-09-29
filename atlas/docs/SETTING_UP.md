# Setting up Atlas

This is the whole thing. You don't need a terminal, a particular folder, or anyone sitting next to you.

## On the laptop

1. **Download `atlas.exe`** and double-click it, wherever it landed. Your Downloads folder is fine.
2. **If Windows says "Windows protected your PC"**, choose **More info**, then **Run anyway**. Atlas isn't signed with a paid certificate, so Windows doesn't recognise it. This happens once: Atlas takes the "downloaded from the internet" mark off its installed copy, so Windows doesn't ask again.
3. **Atlas moves itself into its own home** (`%LOCALAPPDATA%\Atlas`), puts itself in the Start menu and on the desktop, and opens its window. You can delete the `atlas.exe` you downloaded afterwards.
4. **The window sets everything up while you watch:**
   - It fetches what it needs to hear you and talk back, about 330 MB. Every file is checked before it's used, and if the connection drops it picks up where it stopped.
   - It gets to know this computer.
   - It checks itself.
   - Anything worth your attention is listed at the bottom in plain words.
5. **Press Start Atlas.** To have it start whenever you sign in, tick **Start Atlas when I sign in**.

After that, open Atlas from the Start menu or the desktop icon. The same window tells you whether it's running.

## The hub: everything Atlas is tracking

The hub is Atlas's own set of pages: what it's doing now, what's outstanding, what's waiting for your yes, your devices, your connections, recent activity, and your settings. It's the same on the laptop and the phone.

On the laptop, open **Atlas** and press **Hub** at the top of the window, or say "show me the hub". The hub shows right there, inside Atlas's window. It's drawn by the web view that comes with Windows, but it isn't a browser: there's no address bar, and it only ever loads Atlas's own pages from this laptop, so it works with no internet at all. The hub needs the background Atlas running; if it isn't, the Hub page has a **Start Atlas** button.

## Your settings

On the laptop, settings are in Atlas's own window, not a web page:

1. Open **Atlas** from the Start menu or the desktop icon.
2. Press **Settings** at the top of the window. (Or just say "show me settings" to Atlas and the window opens on that page.)
3. Every setting is listed in groups, with what it does and what it costs. Change it right there: a switch, a number, a choice, or a name.
   - A change is kept the moment you make it.
   - Anything that turns on a sensor, reaches outside the laptop or lets Atlas do more without asking asks you once first.
   - **Put back** returns a setting to what Atlas ships with.
4. A running Atlas picks most changes up within a few seconds, with no restart. A few set something up when Atlas starts: the voice, the wake word, the typing key, phone access, the phone companion, the voice's sound and speed, end-of-speech, the model memory budget and identity checks. For those the page says so and shows **Restart Atlas now**. Press it and Atlas stops cleanly, with nothing lost, and starts again with your changes.

On the phone, the same settings are on the hub's **Settings** page in the Atlas phone app.

## On your phone

Atlas reaches your phone over **Tailscale**. It's free for personal use, and it's what keeps Atlas private to your own devices.

1. Install Tailscale on the laptop and on the phone, and sign in to both with the same account.
2. **One time only:** in the Tailscale admin console, open **DNS** and turn on **HTTPS Certificates**. Turn on **MagicDNS** too if it isn't on already. Atlas tells you if this step is missing.
3. Open Atlas on the laptop. Under **Your phone** there's a code. Point the phone's camera at it and open the link.
4. On the phone, choose **Share**, then **Add to Home Screen**. Atlas is now an app on your phone.

The phone app shows Atlas's pages from the laptop. When the laptop can't be reached, it says so and comes back by itself when it can.

## Seeing, and reading pictures

Two optional downloads, each one command (or ATLAS.bat, menu item 8, for the first):

- `atlas get seeing` fetches eight small models, about 133 MB. They let Atlas find faces and tell them apart, name things in front of the camera, follow your hand, and read the words on your screen. Then turn on **Recognising things** in Settings.
- `atlas get pictures` fetches the picture reader, about 3 GB. Then "what does this chart show?" or "look at my screen" gets a spoken answer about what's there. It uses about 3 GB of memory while it answers and none otherwise. The screenshot is deleted once it's been read, and nothing leaves the laptop.

Every file in both is checked against a fingerprint before it's used.

## If something goes wrong

Open Atlas again. The window runs every step again, skips what's already done, and tells you in words what's still missing. There's a **Try the unfinished steps again** button for anything that didn't finish.

## What can still get in the way

- **Smart App Control.** Some Windows 11 machines have this turned on. It blocks programs that aren't signed with a paid certificate, with no "Run anyway" button and no way to make an exception for one program. Atlas checks it during setup and tells you if it's on, or if it's still deciding whether to switch itself on. If it is, switch it off: **Windows Security → App & browser control → Smart App Control settings → Off**. Since the April 2026 Windows update you can switch it back on later without reinstalling Windows. If it blocks Atlas before Atlas can even open, that's the same switch.
- **No internet on first run.** The voice pieces have to be downloaded once. Everything else works offline.
