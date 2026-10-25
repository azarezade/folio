//! Sync, push and delete against a folder posing as a Kindle (mtp-rs's virtual
//! device), so the engine is tested without the real device. The real-device
//! checks are in docs/device-test.md.

use std::fs;
use std::path::{Path, PathBuf};

use folio_device::Kindle;
use folio_library::Layout;
use folio_library::sync::{self, Action, SyncOptions};
use folio_library::transfer::{self, DeleteMode};

const UUID: &str = "180b99f4-123e-1742-272a-a7fd80bab49c";

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("folio-vsync-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake SQLite file of `len` bytes with change counter `counter`.
fn sqlite(len: usize, counter: u32) -> Vec<u8> {
    let mut d = vec![0u8; len];
    d[..16].copy_from_slice(b"SQLite format 3\0");
    d[24..28].copy_from_slice(&counter.to_be_bytes());
    d
}

fn write(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}

fn quiet(_: String) {}

#[test]
fn sync_resync_edit_push_delete() {
    let device = temp("device");
    write(&device, &format!(".notebooks/{UUID}/nbk"), &sqlite(4096, 7));
    write(&device, &format!(".notebooks/{UUID}/actions.log"), b"");
    write(&device, &format!(".notebooks/thumbnails/{UUID}.png"), b"png");
    write(&device, "documents/Papers/paper.pdf", b"%PDF-1.7 paper");
    write(
        &device,
        "documents/B07XYZ1234_EBOK/B07XYZ1234_EBOK.azw3",
        b"kindle book",
    );
    write(&device, "documents/.cache/junk.pdf", b"cache");
    write(&device, "documents/notes.sdr/notes.mbp1", b"sidecar");
    write(&device, "screenshots/Screenshot_2026.png", b"shot");
    let lib = Layout::new(temp("library"));
    let kindle = Kindle::open_virtual(&device).unwrap();

    // first sync: everything is new, in one session
    let plan = sync::plan(&kindle, &lib, &SyncOptions::all(), &mut quiet).unwrap();
    let paths: Vec<String> = plan
        .items
        .iter()
        .map(|i| i.local.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains(&format!("notebooks/{UUID}.nbk")), "{paths:?}");
    assert!(paths.contains(&format!("notebooks/{UUID}.png")));
    assert!(
        paths.contains(&"books/sideloaded/Papers/paper.pdf".to_string()),
        "subfolders are kept: {paths:?}"
    );
    assert!(paths.contains(&"books/purchased/B07XYZ1234_EBOK/B07XYZ1234_EBOK.azw3".to_string()));
    assert!(paths.contains(&"books/sideloaded/notes.sdr/notes.mbp1".to_string()));
    assert!(paths.contains(&"screenshots/Screenshot_2026.png".to_string()));
    assert!(!paths.iter().any(|p| p.contains("junk")), "dot-folders are skipped");
    assert!(plan.items.iter().all(|i| i.action == Action::New));
    let report = sync::run(&kindle, &lib, &plan, &mut |_, _, _| {}, None, &mut quiet).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        fs::read(lib.root.join("books/sideloaded/Papers/paper.pdf")).unwrap(),
        b"%PDF-1.7 paper"
    );

    // re-sync copies nothing
    let again = sync::plan(&kindle, &lib, &SyncOptions::all(), &mut quiet).unwrap();
    assert_eq!(again.to_copy().count(), 0, "{:?}", again.to_copy().collect::<Vec<_>>());
    drop(kindle);

    // an edit on the device that keeps the size is caught by the change counter
    write(&device, &format!(".notebooks/{UUID}/nbk"), &sqlite(4096, 8));
    let kindle = Kindle::open_virtual(&device).unwrap();
    let plan = sync::plan(&kindle, &lib, &SyncOptions::all(), &mut quiet).unwrap();
    let changed: Vec<String> = plan.to_copy().map(|i| i.local.to_string_lossy().into_owned()).collect();
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert!(changed.contains(&format!("notebooks/{UUID}.nbk")));
    assert!(
        changed.contains(&format!("notebooks/{UUID}.png")),
        "the thumbnail follows its notebook"
    );
    sync::run(&kindle, &lib, &plan, &mut |_, _, _| {}, None, &mut quiet).unwrap();
    assert_eq!(
        sync::sqlite_counter(&fs::read(lib.root.join(format!("notebooks/{UUID}.nbk"))).unwrap()),
        Some(8)
    );

    // push twice into a new folder: the second gets " (2)"
    let src = temp("src").join("talk.pdf");
    fs::write(&src, b"%PDF-1.7 talk").unwrap();
    let a = transfer::push(&kindle, &src, "documents/New", &mut |_, _| {}, None).unwrap();
    let b = transfer::push(&kindle, &src, "documents/New", &mut |_, _| {}, None).unwrap();
    assert_eq!(a.device_path, "documents/New/talk.pdf");
    assert_eq!(b.device_path, "documents/New/talk (2).pdf");
    assert!(
        transfer::push(&kindle, &src, "system", &mut |_, _| {}, None).is_err(),
        "only documents/"
    );
    let exe = temp("src").join("tool.exe");
    fs::write(&exe, b"x").unwrap();
    assert!(
        transfer::push(&kindle, &exe, "documents", &mut |_, _| {}, None).is_err(),
        "only Kindle types"
    );

    // sync them, then delete one: device first, then the local copy
    let plan = sync::plan(
        &kindle,
        &lib,
        &SyncOptions {
            books: true,
            ..Default::default()
        },
        &mut quiet,
    )
    .unwrap();
    sync::run(&kindle, &lib, &plan, &mut |_, _, _| {}, None, &mut quiet).unwrap();
    let local = lib.root.join("books/sideloaded/New/talk (2).pdf");
    assert!(local.exists());
    let r = transfer::delete(
        Some(&kindle),
        &lib,
        "documents/New/talk (2).pdf",
        DeleteMode::DeviceThenLocal,
    )
    .unwrap();
    assert!(r.deleted_on_device && r.removed_locally);
    assert!(kindle.resolve("documents/New/talk (2).pdf").unwrap().is_none());
    assert!(!local.exists());

    // notebooks are never deleted, and a file that changed on the device is left alone
    assert!(
        transfer::delete(
            Some(&kindle),
            &lib,
            &format!("notebooks/{UUID}.nbk"),
            DeleteMode::DeviceThenLocal
        )
        .is_err()
    );
    drop(kindle);
    write(&device, "documents/New/talk.pdf", b"%PDF-1.7 a different, longer file");
    let kindle = Kindle::open_virtual(&device).unwrap();
    let err = transfer::delete(
        Some(&kindle),
        &lib,
        "documents/New/talk.pdf",
        DeleteMode::DeviceThenLocal,
    )
    .unwrap_err();
    assert!(err.to_string().contains("not deleting"), "{err}");
    assert!(device.join("documents/New/talk.pdf").exists());
    assert!(
        lib.root.join("books/sideloaded/New/talk.pdf").exists(),
        "local copy kept when the device delete is refused"
    );
}

#[test]
fn python_flattened_books_are_moved_not_downloaded() {
    let device = temp("device-flat");
    write(&device, "documents/dictionaries/Oxford.azw", b"dictionary bytes");
    write(&device, "documents/top.pdf", b"%PDF top");
    let lib = Layout::new(temp("library-flat"));
    // as the Python app left them: no subfolders
    write(&lib.root, "books/sideloaded/Oxford.azw", b"dictionary bytes");
    write(&lib.root, "books/sideloaded/top.pdf", b"%PDF top");
    let kindle = Kindle::open_virtual(&device).unwrap();
    let opts = SyncOptions {
        books: true,
        ..Default::default()
    };
    let plan = sync::plan(&kindle, &lib, &opts, &mut quiet).unwrap();
    let moving: Vec<_> = plan
        .items
        .iter()
        .filter(|i| i.relocate_from.is_some())
        .map(|i| i.local.to_string_lossy().into_owned())
        .collect();
    assert_eq!(moving, vec!["books/sideloaded/dictionaries/Oxford.azw".to_string()]);
    assert_eq!(
        plan.summary()[&sync::Category::Books].bytes_to_copy,
        0,
        "nothing is downloaded"
    );
    sync::run(&kindle, &lib, &plan, &mut |_, _, _| {}, None, &mut quiet).unwrap();
    assert!(lib.root.join("books/sideloaded/dictionaries/Oxford.azw").exists());
    assert!(
        !lib.root.join("books/sideloaded/Oxford.azw").exists(),
        "moved, not copied"
    );
    assert_eq!(
        sync::plan(&kindle, &lib, &opts, &mut quiet).unwrap().to_copy().count(),
        0
    );
}
