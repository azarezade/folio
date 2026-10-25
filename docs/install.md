# Installing Folio

Folio reads, exports and syncs the notebooks, books and documents on a Kindle Scribe. It runs on macOS 12 or later, on both Apple silicon and Intel Macs. It needs nothing else installed: no Homebrew, no Python, no drivers.

## 1. Get the installer

Download `Folio_<version>_universal.dmg` from the [Releases](https://github.com/azarezade/folio/releases) page, or build it yourself from a clone of this repository:

```bash
app/build.sh --universal
```

It takes a few minutes and writes:

- `target/universal-apple-darwin/release/bundle/dmg/Folio_0.1.0_universal.dmg`, the installer (about 16 MB)
- `target/universal-apple-darwin/release/bundle/macos/Folio.app`

Building needs the tools listed in [development.md](development.md). The Mac you install on needs none of them.

## 2. Copy it to the Mac

If you built it on another Mac, copy the `.dmg` over by AirDrop, a USB stick, iCloud Drive or email.

## 3. Install

1. Double-click the `.dmg`.
2. Drag **Folio** onto **Applications**.
3. Eject the disk image.

## 4. Open it the first time

Folio isn't signed with an Apple Developer ID, so macOS blocks the first launch of a copy that was downloaded or came from another Mac.

- **macOS 15 (Sequoia) and later:**
  1. Open Folio from Applications, and click **Done** on the warning.
  2. Go to **System Settings → Privacy & Security**, scroll down, and click **Open Anyway** next to "Folio was blocked".
  3. Enter your password, then click **Open**.
- **macOS 12–14:** in Applications, right-click (or Control-click) **Folio**, choose **Open**, then click **Open** again.

You only have to do this once. Alternatively, run this in Terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Folio.app
```

## 5. Set up your library

Folio keeps everything in **`~/Library/Application Support/Folio`**: notebooks, books, PDFs, screenshots, reading positions and your notebook names. Pick one of these:

- **Start fresh.** Plug in the Kindle and click **Sync**. The first sync copies everything, about 1 GB on a well-used Scribe and roughly a minute over USB. Later syncs copy only what changed.
- **Bring your library from another Mac** (keeps notebook names and reading positions):
  1. On the old Mac, copy the library folder, `~/Library/Application Support/Folio` (or the folder you pointed `FOLIO_LIBRARY` at).
  2. On the new Mac, quit Folio.
  3. Put the folder's contents into `~/Library/Application Support/Folio`. In Finder, press ⇧⌘G and paste that path; create the `Folio` folder if it isn't there yet.
  4. Open Folio again; it finds everything on its own.
- **Import a library from the earlier [scribe-library](https://github.com/technolaaji/scribe-library) Python app:** go to **Settings (⌘,) › General › Import an old library…** and choose its `library` folder. Files Folio already has are skipped.

The render cache is in `~/Library/Caches/Folio`. It's safe to delete; pages are drawn again when needed.

## 6. Connect the Kindle

1. Plug the Kindle Scribe in with a USB cable, then wake and unlock it.
2. It appears in the sidebar under **Kindle** within a few seconds.

If the sidebar says **"Held by another process"**, macOS's photo service (Image Capture, `ptpcamerad`) grabbed the USB connection first. Click the Kindle row, then **Free the Kindle**. To stop it from happening again, turn on **Settings › Sync › Stop macOS from grabbing the Kindle**. That stops Image Capture from importing from cameras and phones over USB until you turn it back off.

Optional, in **Settings › Sync**:

- **Sync automatically when the Kindle is plugged in.**
- **Also copy the Kindle's sync database.** It's about 1 GB and copied again whenever it changes. Leave it off unless you need it.

## Updating

Build or get a newer `.dmg` and drag **Folio** into Applications again, choosing **Replace**. Your library isn't touched. You may need to repeat step 4 once.

## Removing

1. Drag **Folio** from Applications to the Bin.
2. To remove its data too, delete `~/Library/Application Support/Folio` and `~/Library/Caches/Folio`.

Your notebooks and books stay on the Kindle either way.

## Licences

**Folio › About Folio** shows the GPL notice and a link to the source code. **Help › Licences** lists every bundled component.

## Notes

- Earlier builds were called "Scribe Library". Their data folder `~/Library/Application Support/Scribe Library` is renamed to `Folio` the first time Folio starts.
- The environment variables `FOLIO_LIBRARY` (or the older `SCRIBE_LIBRARY`) point Folio at a different library folder, for testing.
