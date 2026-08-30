use std::{
    fs::{self, File},
    hint::black_box,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_json::json;

use crate::{app, dxvk, enhanced, fs_ops, models::DxvkSettings, steam, t7};

const ARCHIVE_SAMPLE_COUNT: usize = 7;
const ARCHIVE_FILE_COUNT: usize = 4;
const ARCHIVE_FILE_BYTES: usize = 1024 * 1024;
const TRANSACTION_SAMPLE_COUNT: usize = 7;
const STATUS_BATCH_ITERATIONS: usize = 1_000;

fn samples_ms(samples: &[Duration]) -> Vec<f64> {
    samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect()
}

fn median_ms(samples: &[Duration]) -> f64 {
    let mut samples = samples.to_vec();
    samples.sort_unstable();
    samples[samples.len() / 2].as_secs_f64() * 1_000.0
}

fn median_f64(samples: &[f64]) -> f64 {
    let mut samples = samples.to_vec();
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let value = operation();
    (start.elapsed(), value)
}

fn batched_ms(iterations: usize, operation: impl Fn()) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        operation();
    }
    start.elapsed().as_secs_f64() * 1_000.0 / iterations as f64
}

fn deterministic_byte(state: &mut u64) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 24) as u8
}

fn write_deterministic_payload(
    writer: &mut impl Write,
    bytes: usize,
    mut state: u64,
) -> std::io::Result<()> {
    let mut buffer = vec![0_u8; ARCHIVE_FILE_BYTES.min(bytes.max(1))];
    let mut remaining = bytes;
    while remaining > 0 {
        let length = remaining.min(buffer.len());
        for byte in &mut buffer[..length] {
            *byte = deterministic_byte(&mut state);
        }
        writer.write_all(&buffer[..length])?;
        remaining -= length;
    }
    Ok(())
}

fn deterministic_archive_payload() -> Vec<u8> {
    let mut payload = Vec::with_capacity(ARCHIVE_FILE_COUNT * ARCHIVE_FILE_BYTES);
    write_deterministic_payload(
        &mut payload,
        ARCHIVE_FILE_COUNT * ARCHIVE_FILE_BYTES,
        0x4d59_5df4_d0f3_3173,
    )
    .expect("build deterministic archive payload");
    payload
}

fn write_deterministic_file(path: &Path, bytes: usize, seed: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create deterministic payload directory");
    }
    let mut file = File::create(path).expect("create deterministic payload file");
    write_deterministic_payload(&mut file, bytes, seed).expect("write deterministic payload file");
}

fn create_archive_fixtures(root: &Path) -> (PathBuf, PathBuf) {
    if root.exists() {
        fs::remove_dir_all(root).expect("remove old benchmark archives");
    }
    fs::create_dir_all(root).expect("create benchmark archive directory");
    let payload = deterministic_archive_payload();
    let (payload_chunks, remainder) = payload.as_chunks::<ARCHIVE_FILE_BYTES>();
    assert!(remainder.is_empty(), "benchmark payload must divide evenly");

    let zip_path = root.join("representative-deflate.zip");
    let mut zip = zip::ZipWriter::new(File::create(&zip_path).expect("create ZIP fixture"));
    let zip_options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));
    for (index, chunk) in payload_chunks.iter().enumerate() {
        zip.start_file(format!("payload/file-{index}.bin"), zip_options)
            .expect("start ZIP fixture entry");
        zip.write_all(chunk).expect("write ZIP fixture entry");
    }
    zip.finish().expect("finish ZIP fixture");

    let tar_gz_path = root.join("representative.tar.gz");
    let encoder = flate2::write::GzEncoder::new(
        File::create(&tar_gz_path).expect("create tar.gz fixture"),
        flate2::Compression::new(6),
    );
    let mut tar = tar::Builder::new(encoder);
    let mut directory = tar::Header::new_gnu();
    directory.set_entry_type(tar::EntryType::Directory);
    directory.set_size(0);
    directory.set_mode(0o755);
    directory.set_uid(0);
    directory.set_gid(0);
    directory.set_mtime(0);
    directory.set_cksum();
    tar.append_data(&mut directory, "payload", Cursor::new([]))
        .expect("write tar.gz fixture directory");
    for (index, chunk) in payload_chunks.iter().enumerate() {
        let mut header = tar::Header::new_gnu();
        header.set_size(chunk.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(
            &mut header,
            format!("payload/file-{index}.bin"),
            Cursor::new(chunk),
        )
        .expect("write tar.gz fixture entry");
    }
    tar.into_inner()
        .expect("finish tar fixture")
        .finish()
        .expect("finish gzip fixture");

    (zip_path, tar_gz_path)
}

fn extraction_samples(
    archive: &Path,
    destination_root: &Path,
    destination_prefix: &str,
    extract: fn(&Path, &Path) -> Result<(), String>,
) -> Vec<Duration> {
    (0..ARCHIVE_SAMPLE_COUNT)
        .map(|sample| {
            let destination = destination_root.join(format!("{destination_prefix}-{sample}"));
            if destination.exists() {
                fs::remove_dir_all(&destination).expect("clean benchmark extraction destination");
            }
            drop_file_cache(archive);
            let (elapsed, result) = timed(|| extract(archive, &destination));
            result.expect("extract benchmark archive");
            for index in 0..ARCHIVE_FILE_COUNT {
                assert_eq!(
                    fs::metadata(destination.join(format!("payload/file-{index}.bin")))
                        .expect("benchmark extracted file")
                        .len(),
                    ARCHIVE_FILE_BYTES as u64
                );
            }
            fs::remove_dir_all(destination).expect("remove benchmark extraction destination");
            elapsed
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn drop_file_cache(path: &Path) {
    use std::{fs::File, os::fd::AsRawFd};

    unsafe extern "C" {
        fn posix_fadvise(fd: i32, offset: i64, len: i64, advice: i32) -> i32;
    }

    let file = File::open(path).expect("benchmark cache target");
    let result = unsafe { posix_fadvise(file.as_raw_fd(), 0, 0, 4) };
    assert_eq!(result, 0, "POSIX_FADV_DONTNEED failed");
}

#[cfg(not(target_os = "linux"))]
fn drop_file_cache(_: &Path) {}

struct RestoreFile {
    path: PathBuf,
    body: Vec<u8>,
}

impl Drop for RestoreFile {
    fn drop(&mut self) {
        fs::write(&self.path, &self.body).expect("restore benchmark fixture");
    }
}

#[derive(Default)]
struct TransactionSamples {
    install: Vec<Duration>,
    configure: Vec<Duration>,
    status: Vec<f64>,
    uninstall: Vec<Duration>,
}

fn create_t7_transaction_sources(root: &Path) -> Vec<(PathBuf, PathBuf)> {
    let files = [
        ("dsound.dll", 616_960),
        ("t7patch.dll", 171_008),
        ("t7patchloader.dll", 136_192),
        ("LPC/bp_core_ffotd_tu32_593.ff", 960),
        ("LPC/core_ffotd_tu32_593.ff", 185_856),
        ("LPC/ea_core_ffotd_tu32_593.ff", 960),
        ("LPC/en_core_ffotd_tu32_593.ff", 960),
        ("LPC/es_core_ffotd_tu32_593.ff", 960),
        ("LPC/fr_core_ffotd_tu32_593.ff", 960),
        ("LPC/ge_core_ffotd_tu32_593.ff", 960),
        ("LPC/it_core_ffotd_tu32_593.ff", 960),
        ("LPC/ja_core_ffotd_tu32_593.ff", 960),
        ("LPC/po_core_ffotd_tu32_593.ff", 960),
        ("LPC/ru_core_ffotd_tu32_593.ff", 960),
        ("LPC/sc_core_ffotd_tu32_593.ff", 960),
        ("LPC/tc_core_ffotd_tu32_593.ff", 960),
    ];
    let source_root = root.join("t7");
    let mut sources = files
        .iter()
        .enumerate()
        .map(|(index, (relative, bytes))| {
            let relative = PathBuf::from(relative);
            let source = source_root.join(&relative);
            write_deterministic_file(
                &source,
                *bytes,
                0x7430_0000_0000_0001_u64.wrapping_add(index as u64),
            );
            (source, relative)
        })
        .collect::<Vec<_>>();
    let config_relative = PathBuf::from("t7patch.conf");
    let config_source = source_root.join(&config_relative);
    fs::write(
        &config_source,
        b"playername=Benchmark\nnetworkpassword=\nisfriendsonly=0\n",
    )
    .expect("write T7 transaction config");
    sources.push((config_source, config_relative));
    sources
}

fn create_dxvk_transaction_sources(root: &Path) -> [PathBuf; 2] {
    let source_root = root.join("dxvk");
    let dxgi = source_root.join("dxgi.dll");
    let d3d11 = source_root.join("d3d11.dll");
    write_deterministic_file(&dxgi, 5_357_582, 0x6478_766b_0000_0001);
    write_deterministic_file(&d3d11, 7_397_390, 0x6478_766b_0000_0002);
    [dxgi, d3d11]
}

fn create_enhanced_transaction_sources(
    root: &Path,
    executable: &Path,
) -> (PathBuf, PathBuf, PathBuf) {
    let archive = root.join(enhanced::ENHANCED_ARCHIVE_NAME);
    let mut zip = zip::ZipWriter::new(File::create(&archive).expect("create Enhanced archive"));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));
    for (index, (name, bytes)) in [
        ("T7WSBootstrapper.dll", 144_384),
        ("T7InternalWS.dll", 394_752),
        ("steam_api65.dll", 319_584),
        ("WindowsCodecs.dll", 1_768_960),
    ]
    .iter()
    .enumerate()
    {
        zip.start_file(*name, options)
            .expect("start Enhanced archive entry");
        write_deterministic_payload(
            &mut zip,
            *bytes,
            0x656e_6861_6e63_0001_u64.wrapping_add(index as u64),
        )
        .expect("write Enhanced archive entry");
    }
    zip.finish().expect("finish Enhanced archive");

    let dump = root.join("enhanced-dump");
    fs::create_dir_all(&dump).expect("create Enhanced dump fixture");
    let dump_executable = dump.join("BlackOps3.exe");
    if fs::hard_link(executable, &dump_executable).is_err() {
        fs::copy(executable, &dump_executable).expect("copy Enhanced dump executable");
    }
    fs::write(
        dump.join("MicrosoftGame.config"),
        b"<Game Identity=\"PatchOpsBenchmark\" />\n",
    )
    .expect("write Enhanced dump config");
    write_deterministic_file(
        &dump.join("GameChat2.dll"),
        1024 * 1024,
        0x656e_6861_6e63_1001,
    );
    let original_executable = root.join("enhanced-original-BlackOps3.exe");
    write_deterministic_file(
        &original_executable,
        fs::metadata(executable).unwrap().len() as usize,
        0x656e_6861_6e63_2001,
    );
    (archive, dump, original_executable)
}

fn create_compatibility_transaction_source(root: &Path) -> PathBuf {
    let source = root.join("compatibility-tool");
    write_deterministic_file(&source.join("proton"), 1024 * 1024, 0x7072_6f74_6f6e_0001);
    write_deterministic_file(
        &source.join("files/payload"),
        4 * 1024 * 1024,
        0x7072_6f74_6f6e_0002,
    );
    source
}

fn t7_transaction_samples(root: &Path, sources: &[(PathBuf, PathBuf)]) -> TransactionSamples {
    let mut samples = TransactionSamples::default();
    let original_dsound = b"original benchmark dsound";
    let original_lpc = b"original benchmark LPC";
    let configured =
        b"unknown=keep\nplayername=^2Benchmark\nnetworkpassword=offline\nisfriendsonly=1\n";

    for sample in 0..TRANSACTION_SAMPLE_COUNT {
        let sample_root = root.join(format!("t7-{sample}"));
        let game = sample_root.join("game");
        let rollback = sample_root.join("rollback");
        fs::create_dir_all(game.join("LPC")).expect("create T7 transaction game");
        fs::write(game.join("dsound.dll"), original_dsound).expect("write original T7 DLL");
        fs::write(game.join("LPC/en_core_ffotd_tu32_593.ff"), original_lpc)
            .expect("write original T7 LPC");
        fs::write(game.join("unrelated.bin"), b"preserve").expect("write T7 transaction sentinel");

        let (elapsed, result) =
            timed(|| t7::benchmark_install_transaction(&game, sources, &rollback.join("install")));
        result.expect("install T7 transaction fixture");
        samples.install.push(elapsed);
        let installed = t7::status(Some(&game), Some("current"));
        assert!(installed.installed && installed.conf_exists);

        let (elapsed, result) = timed(|| {
            t7::benchmark_configure_transaction(&game, configured, &rollback.join("configure"))
        });
        result.expect("configure T7 transaction fixture");
        samples.configure.push(elapsed);
        let configured_state = t7::status(Some(&game), Some("current"));
        assert_eq!(configured_state.plain_name, "Benchmark");
        assert_eq!(configured_state.network_password, "offline");
        assert!(configured_state.friends_only);
        samples.status.push(batched_ms(STATUS_BATCH_ITERATIONS, || {
            black_box(t7::status(Some(&game), Some("current")));
        }));

        let (elapsed, result) =
            timed(|| t7::benchmark_uninstall_transaction(&game, &rollback.join("uninstall")));
        result.expect("uninstall T7 transaction fixture");
        samples.uninstall.push(elapsed);

        assert_eq!(fs::read(game.join("dsound.dll")).unwrap(), original_dsound);
        assert_eq!(
            fs::read(game.join("LPC/en_core_ffotd_tu32_593.ff")).unwrap(),
            original_lpc
        );
        for (_, relative) in sources {
            if relative != Path::new("dsound.dll")
                && relative != Path::new("LPC/en_core_ffotd_tu32_593.ff")
            {
                assert!(!game.join(relative).exists());
            }
        }
        assert!(!game.join(".patchopsiii").exists());
        assert_eq!(fs::read(game.join("unrelated.bin")).unwrap(), b"preserve");
        fs::remove_dir_all(sample_root).expect("remove T7 transaction sample");
    }
    samples
}

fn dxvk_transaction_samples(root: &Path, sources: &[PathBuf; 2]) -> TransactionSamples {
    let mut samples = TransactionSamples::default();
    let original_config = b"# original benchmark config\nunrelated=value\n";
    let installed_settings = DxvkSettings::default();
    let configured_settings = DxvkSettings {
        enable_async: false,
        gpl_async_cache: true,
        num_compiler_threads: 4,
        max_frame_rate: 240,
        max_frame_latency: 2,
        tear_free: "False".into(),
        hud_enabled: true,
    };

    for sample in 0..TRANSACTION_SAMPLE_COUNT {
        let sample_root = root.join(format!("dxvk-{sample}"));
        let game = sample_root.join("game");
        let storage = sample_root.join("storage");
        let staging = sample_root.join("staging");
        fs::create_dir_all(&game).expect("create DXVK transaction game");
        fs::create_dir_all(&storage).expect("create DXVK transaction storage");
        fs::write(game.join("dxvk.conf"), original_config).expect("write original DXVK config");
        fs::write(game.join("unrelated.bin"), b"preserve")
            .expect("write DXVK transaction sentinel");

        let (elapsed, result) = timed(|| {
            dxvk::benchmark_install_transaction(
                &storage,
                &game,
                sources,
                &installed_settings,
                &sample_root.join("rollback/install"),
            )
        });
        result.expect("install DXVK transaction fixture");
        samples.install.push(elapsed);
        assert!(dxvk::status(Some(&game)).installed);

        let (elapsed, result) = timed(|| {
            dxvk::benchmark_configure_transaction(&storage, &game, &configured_settings, &staging)
        });
        result.expect("configure DXVK transaction fixture");
        samples.configure.push(elapsed);
        let configured_state = dxvk::status(Some(&game));
        assert_eq!(configured_state.settings, configured_settings);
        samples.status.push(batched_ms(STATUS_BATCH_ITERATIONS, || {
            black_box(dxvk::status(Some(&game)));
        }));

        let (elapsed, result) = timed(|| dxvk::benchmark_uninstall_transaction(&storage, &game));
        assert!(result.expect("uninstall DXVK transaction fixture"));
        samples.uninstall.push(elapsed);

        assert!(!game.join("dxgi.dll").exists());
        assert!(!game.join("d3d11.dll").exists());
        assert_eq!(fs::read(game.join("dxvk.conf")).unwrap(), original_config);
        assert!(!storage.join("DXVK Managed").exists());
        assert_eq!(fs::read(game.join("unrelated.bin")).unwrap(), b"preserve");
        fs::remove_dir_all(sample_root).expect("remove DXVK transaction sample");
    }
    samples
}

fn enhanced_transaction_samples(
    root: &Path,
    archive: &Path,
    dump: &Path,
    original_executable: &Path,
) -> TransactionSamples {
    let mut samples = TransactionSamples::default();
    let original_config = b"original benchmark game config";
    let original_executable_hash =
        fs_ops::sha256_file(original_executable).expect("hash original Enhanced executable");

    for sample in 0..TRANSACTION_SAMPLE_COUNT {
        let sample_root = root.join(format!("enhanced-{sample}"));
        let game = sample_root.join("game");
        let storage = sample_root.join("storage");
        fs::create_dir_all(&game).expect("create Enhanced transaction game");
        fs::create_dir_all(&storage).expect("create Enhanced transaction storage");
        let game_executable = game.join("BlackOps3.exe");
        if fs::hard_link(original_executable, &game_executable).is_err() {
            fs::copy(original_executable, &game_executable)
                .expect("copy original Enhanced executable");
        }
        fs::write(game.join("MicrosoftGame.config"), original_config)
            .expect("write original Enhanced config");
        fs::write(game.join("unrelated.bin"), b"preserve")
            .expect("write Enhanced transaction sentinel");
        enhanced::benchmark_record_cached_archive(&storage, archive)
            .expect("record Enhanced transaction checksum");

        let (elapsed, result) =
            timed(|| enhanced::benchmark_install_transaction(&game, archive, dump, &storage));
        result.expect("install Enhanced transaction fixture");
        samples.install.push(elapsed);
        assert!(enhanced::detect_install(&game));
        assert_eq!(
            fs::metadata(game.join("BlackOps3.exe")).unwrap().len(),
            fs::metadata(dump.join("BlackOps3.exe")).unwrap().len()
        );
        assert_eq!(
            fs::metadata(game.join("WindowsCodecs.dll")).unwrap().len(),
            1_768_960
        );

        let state = enhanced::benchmark_status_transaction(
            &storage,
            &game,
            steam::ENHANCED_LAUNCH_OPTIONS,
            dump,
        )
        .expect("read Enhanced transaction status");
        assert!(state.installed && state.launch_options_active);
        assert_eq!(state.files_installed, 7);
        assert_eq!(state.backup_status, "Created");
        assert_eq!(state.dump_source, dump.to_string_lossy());
        samples.status.push(batched_ms(STATUS_BATCH_ITERATIONS, || {
            black_box(
                enhanced::benchmark_status_transaction(
                    &storage,
                    &game,
                    steam::ENHANCED_LAUNCH_OPTIONS,
                    dump,
                )
                .expect("read warm Enhanced transaction status"),
            );
        }));

        let (elapsed, result) =
            timed(|| enhanced::benchmark_uninstall_transaction(&game, &storage));
        result.expect("uninstall Enhanced transaction fixture");
        samples.uninstall.push(elapsed);

        assert_eq!(
            fs_ops::sha256_file(&game_executable).unwrap(),
            original_executable_hash
        );
        assert_eq!(
            fs::read(game.join("MicrosoftGame.config")).unwrap(),
            original_config
        );
        for created in [
            "T7WSBootstrapper.dll",
            "T7InternalWS.dll",
            "steam_api65.dll",
            "WindowsCodecs.dll",
            "GameChat2.dll",
        ] {
            assert!(!game.join(created).exists());
        }
        let clean_state = enhanced::benchmark_status_transaction(&storage, &game, "-novid", dump)
            .expect("read uninstalled Enhanced transaction status");
        assert!(!clean_state.installed);
        assert_eq!(clean_state.files_installed, 0);
        for entry in fs::read_dir(&game).expect("inspect Enhanced transaction cleanup") {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.starts_with(".patchops-enhanced-"));
            assert!(!name.contains(".patchops.original-"));
        }
        assert_eq!(fs::read(game.join("unrelated.bin")).unwrap(), b"preserve");
        fs::remove_dir_all(sample_root).expect("remove Enhanced transaction sample");
    }
    samples
}

fn compatibility_transaction_samples(root: &Path, source: &Path) -> (Vec<Duration>, Vec<Duration>) {
    let mut configure = Vec::new();
    let mut cleanup = Vec::new();
    let original_steam_config = r#""InstallConfigStore" {
  "Unrelated" "preserve"
  "Software" { "Valve" { "Steam" { "CompatToolMapping" {
    "311210" { "name" "GE-Proton" "priority" "75" "custom" "yes" }
  } } } }
}"#;
    let original_local_config = r#""UserLocalConfigStore" {
  "Software" { "Valve" { "Steam" { "apps" {
    "311210" { "LaunchOptions" "-novid" }
  } } } }
}"#;

    for sample in 0..TRANSACTION_SAMPLE_COUNT {
        let sample_root = root.join(format!("compatibility-{sample}"));
        let steam_root = sample_root.join("steam");
        let data_dir = sample_root.join("data");
        let steam_config = steam_root.join("config/config.vdf");
        let local_config = steam_root.join("userdata/1/config/localconfig.vdf");
        fs::create_dir_all(steam_config.parent().unwrap())
            .expect("create compatibility Steam config directory");
        fs::create_dir_all(local_config.parent().unwrap())
            .expect("create compatibility local config directory");
        fs::create_dir_all(&data_dir).expect("create compatibility data directory");
        fs::write(&steam_config, original_steam_config).expect("write compatibility Steam config");
        fs::write(&local_config, original_local_config).expect("write compatibility local config");

        let (elapsed, result) = timed(|| {
            steam::benchmark_configure_compatibility_transaction(
                &steam_root,
                source,
                &data_dir,
                &local_config,
            )
        });
        let destination = result.expect("configure compatibility transaction fixture");
        configure.push(elapsed);
        assert!(destination.join("proton").is_file());
        assert!(destination.join("files/payload").is_file());
        assert!(
            !steam::benchmark_compatibility_is_clean(&steam_root, &data_dir, &local_config,)
                .expect("inspect configured compatibility transaction")
        );
        assert!(fs::read_to_string(&steam_config)
            .unwrap()
            .contains(steam::ENHANCED_TOOL_NAME));
        assert!(fs::read_to_string(&local_config)
            .unwrap()
            .contains("WindowsCodecs=n,b"));

        let (elapsed, result) = timed(|| {
            steam::benchmark_cleanup_compatibility_transaction(
                &steam_root,
                &data_dir,
                &local_config,
            )
        });
        result.expect("cleanup compatibility transaction fixture");
        cleanup.push(elapsed);
        assert!(
            steam::benchmark_compatibility_is_clean(&steam_root, &data_dir, &local_config,)
                .expect("inspect cleaned compatibility transaction")
        );
        let restored_steam = fs::read_to_string(&steam_config).unwrap();
        assert!(restored_steam.contains("GE-Proton"));
        assert!(restored_steam.contains("custom"));
        assert!(restored_steam.contains("preserve"));
        assert!(fs::read_to_string(&local_config)
            .unwrap()
            .contains("-novid"));
        assert!(!destination.exists());
        fs::remove_dir_all(sample_root).expect("remove compatibility transaction sample");
    }
    (configure, cleanup)
}

#[test]
#[ignore = "explicit fixed-fixture performance benchmark"]
fn fixed_fixture_operations() {
    let fixture = std::env::var_os("PATCHOPSIII_BENCHMARK_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/patchops-benchmark-fixture"));
    let game = fixture.join("game");
    let steam_root = fixture.join("steam");
    let library_vdf = steam_root.join("steamapps/libraryfolders.vdf");
    let config = app::config_path(&game);
    let executable = game.join("BlackOps3.exe");
    assert!(library_vdf.is_file() && config.is_file() && executable.is_file());

    let archive_fixture = fixture.join(format!("archive-benchmark-{}", std::process::id()));
    let (zip_archive, tar_gz_archive) = create_archive_fixtures(&archive_fixture);
    let zip_extract = extraction_samples(
        &zip_archive,
        &archive_fixture,
        "zip-extract",
        fs_ops::extract_zip,
    );
    let tar_gz_extract = extraction_samples(
        &tar_gz_archive,
        &archive_fixture,
        "tar-gz-extract",
        fs_ops::extract_tar_gz,
    );

    let transaction_fixture = fixture.join(format!("transaction-benchmark-{}", std::process::id()));
    if transaction_fixture.exists() {
        fs::remove_dir_all(&transaction_fixture).expect("remove old transaction benchmark fixture");
    }
    let transaction_sources = transaction_fixture.join("sources");
    fs::create_dir_all(&transaction_sources).expect("create transaction benchmark sources");
    let t7_sources = create_t7_transaction_sources(&transaction_sources);
    let dxvk_sources = create_dxvk_transaction_sources(&transaction_sources);
    let (enhanced_archive, enhanced_dump, enhanced_original) =
        create_enhanced_transaction_sources(&transaction_sources, &executable);
    let compatibility_source = create_compatibility_transaction_source(&transaction_sources);
    let transaction_samples = transaction_fixture.join("samples");
    let t7_transactions = t7_transaction_samples(&transaction_samples, &t7_sources);
    let dxvk_transactions = dxvk_transaction_samples(&transaction_samples, &dxvk_sources);
    let enhanced_transactions = enhanced_transaction_samples(
        &transaction_samples,
        &enhanced_archive,
        &enhanced_dump,
        &enhanced_original,
    );
    let compatibility_transactions = cfg!(target_os = "linux")
        .then(|| compatibility_transaction_samples(&transaction_samples, &compatibility_source));

    let mut steam_cold = Vec::new();
    for _ in 0..15 {
        drop_file_cache(&library_vdf);
        let (elapsed, paths) = timed(|| steam::library_paths_from_roots(vec![steam_root.clone()]));
        black_box(paths);
        steam_cold.push(elapsed);
    }
    let steam_warm = (0..7)
        .map(|_| {
            batched_ms(1_000, || {
                black_box(steam::library_paths_from_roots(vec![steam_root.clone()]));
            })
        })
        .collect::<Vec<_>>();

    let game_text = game.to_string_lossy();
    let mut game_cold = Vec::new();
    for _ in 0..15 {
        drop_file_cache(&executable);
        let (elapsed, found) = timed(|| steam::find_game_directory(Some(&game_text)));
        black_box(found);
        game_cold.push(elapsed);
    }
    let game_warm = (0..7)
        .map(|_| {
            batched_ms(1_000, || {
                black_box(steam::find_game_directory(Some(&game_text)));
            })
        })
        .collect::<Vec<_>>();

    let config_read = (0..7)
        .map(|_| {
            batched_ms(1_000, || {
                black_box(app::read_config(&game));
            })
        })
        .collect::<Vec<_>>();

    let original_config = fs::read(&config).expect("benchmark config");
    let _restore = RestoreFile {
        path: config.clone(),
        body: original_config,
    };
    let config_write = (0..21)
        .map(|index| {
            let value = if index % 2 == 0 { "239" } else { "240" };
            let updates = vec![("MaxFPS".into(), value.into(), "Maximum FPS cap".into())];
            let (elapsed, result) = timed(|| app::write_config_values(&game, &updates));
            result.expect("benchmark config write");
            elapsed
        })
        .collect::<Vec<_>>();

    let mut sha_cold = Vec::new();
    for _ in 0..5 {
        drop_file_cache(&executable);
        let (elapsed, digest) = timed(|| fs_ops::sha256_file(&executable));
        black_box(digest.expect("benchmark executable hash"));
        sha_cold.push(elapsed);
    }
    let sha_warm = (0..5)
        .map(|_| {
            let (elapsed, digest) = timed(|| fs_ops::sha256_file(&executable));
            black_box(digest.expect("benchmark executable hash"));
            elapsed
        })
        .collect::<Vec<_>>();

    let component_status = (0..7)
        .map(|_| {
            batched_ms(1_000, || {
                black_box(t7::status(Some(&game), Some("current")));
                black_box(dxvk::status(Some(&game)));
                black_box(app::qol_state(Some(&game)));
                let body = app::read_config(&game);
                black_box(app::graphics_state(&body));
                black_box(app::advanced_state(Some(&game), &body));
            })
        })
        .collect::<Vec<_>>();

    let output = json!({
        "fixture": fixture,
        "samplesMs": {
            "steamDetectionCold": samples_ms(&steam_cold),
            "steamDetectionWarm": &steam_warm,
            "gameDetectionCold": samples_ms(&game_cold),
            "gameDetectionWarm": &game_warm,
            "configRead": &config_read,
            "configWrite": samples_ms(&config_write),
            "executableSha256Cold": samples_ms(&sha_cold),
            "executableSha256Warm": samples_ms(&sha_warm),
            "componentStatusWarm": &component_status,
            "zipDeflateExtract": samples_ms(&zip_extract),
            "tarGzExtract": samples_ms(&tar_gz_extract),
            "t7InstallTransactionCore": samples_ms(&t7_transactions.install),
            "t7ConfigureTransactionCore": samples_ms(&t7_transactions.configure),
            "t7StatusTransactionCoreWarm": &t7_transactions.status,
            "t7UninstallTransactionCore": samples_ms(&t7_transactions.uninstall),
            "dxvkInstallTransactionCore": samples_ms(&dxvk_transactions.install),
            "dxvkConfigureTransactionCore": samples_ms(&dxvk_transactions.configure),
            "dxvkStatusTransactionCoreWarm": &dxvk_transactions.status,
            "dxvkUninstallTransactionCore": samples_ms(&dxvk_transactions.uninstall),
            "enhancedInstallTransactionCore": samples_ms(&enhanced_transactions.install),
            "enhancedStatusTransactionCoreWarm": &enhanced_transactions.status,
            "enhancedUninstallTransactionCore": samples_ms(&enhanced_transactions.uninstall),
            "enhancedLinuxCompatibilityConfigureTransactionCore": compatibility_transactions
                .as_ref()
                .map(|samples| samples_ms(&samples.0)),
            "enhancedLinuxCompatibilityCleanupTransactionCore": compatibility_transactions
                .as_ref()
                .map(|samples| samples_ms(&samples.1))
        },
        "mediansMs": {
            "steamDetectionCold": median_ms(&steam_cold),
            "steamDetectionWarm": median_f64(&steam_warm),
            "gameDetectionCold": median_ms(&game_cold),
            "gameDetectionWarm": median_f64(&game_warm),
            "configRead": median_f64(&config_read),
            "configWrite": median_ms(&config_write),
            "executableSha256Cold": median_ms(&sha_cold),
            "executableSha256Warm": median_ms(&sha_warm),
            "componentStatusWarm": median_f64(&component_status),
            "zipDeflateExtract": median_ms(&zip_extract),
            "tarGzExtract": median_ms(&tar_gz_extract),
            "t7InstallTransactionCore": median_ms(&t7_transactions.install),
            "t7ConfigureTransactionCore": median_ms(&t7_transactions.configure),
            "t7StatusTransactionCoreWarm": median_f64(&t7_transactions.status),
            "t7UninstallTransactionCore": median_ms(&t7_transactions.uninstall),
            "dxvkInstallTransactionCore": median_ms(&dxvk_transactions.install),
            "dxvkConfigureTransactionCore": median_ms(&dxvk_transactions.configure),
            "dxvkStatusTransactionCoreWarm": median_f64(&dxvk_transactions.status),
            "dxvkUninstallTransactionCore": median_ms(&dxvk_transactions.uninstall),
            "enhancedInstallTransactionCore": median_ms(&enhanced_transactions.install),
            "enhancedStatusTransactionCoreWarm": median_f64(&enhanced_transactions.status),
            "enhancedUninstallTransactionCore": median_ms(&enhanced_transactions.uninstall),
            "enhancedLinuxCompatibilityConfigureTransactionCore": compatibility_transactions
                .as_ref()
                .map(|samples| median_ms(&samples.0)),
            "enhancedLinuxCompatibilityCleanupTransactionCore": compatibility_transactions
                .as_ref()
                .map(|samples| median_ms(&samples.1))
        }
    });
    fs::remove_dir_all(archive_fixture).expect("remove benchmark archive fixtures");
    fs::remove_dir_all(transaction_fixture).expect("remove transaction benchmark fixtures");
    println!("PATCHOPSIII_OPERATION_BENCHMARK_JSON={output}");
}
