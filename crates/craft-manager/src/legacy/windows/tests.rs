use super::*;
use crate::APPS;
use std::cell::Cell;

const PRODUCT: &str = "{12345678-1234-5678-90AB-CDEF12345678}";

fn app() -> App {
    APPS.first().copied().expect("catalog")
}

fn entry(app: App) -> RegistryEntry {
    RegistryEntry {
        name: app.name.to_owned(),
        publisher: PUBLISHER.into(),
        version: Some("0.1.0".into()),
        location: Some(format!(r"C:\Program Files\{}", app.name)),
        windows_installer: 1,
    }
}

fn install() -> LegacyInstall {
    identify(app(), Hive::LocalMachine, KEY_WOW64_64KEY, PRODUCT.into(), entry(app())).expect("MSI fixture")
}

#[test]
fn recognizes_each_audited_package_in_all_hives_and_views() {
    for app in APPS {
        for hive in [Hive::CurrentUser, Hive::LocalMachine] {
            for view in REGISTRY_VIEWS {
                for publisher in [PUBLISHER, LEGACY_PUBLISHER] {
                    let mut record = entry(app);
                    record.publisher = publisher.into();
                    let detected = identify(app, hive, view, PRODUCT.into(), record).expect("MSI");
                    assert_eq!(detected.app, app);
                    assert_eq!(detected.version.as_deref(), Some("0.1.0"));
                    assert_eq!(detected.method, LegacyMethod::WindowsMsi);
                    assert_eq!(detected.location, Some(PathBuf::from(format!(r"C:\Program Files\{}", app.name))));
                    assert!(detected.description.contains(hive.label()));
                }
            }
        }
    }
}

#[test]
fn refuses_similar_names_other_publishers_non_msi_and_hub() {
    for name in ["Craftworks", "PdfCraft Helper", "PdfCraft SDK", "NotPdfCraft", "PdfCraft Pro", "PdfCraft 0.2.0", "PdfCraft 0.1.0 extra"] {
        let mut record = entry(app());
        record.name = name.into();
        assert!(identify(app(), Hive::LocalMachine, KEY_WOW64_64KEY, PRODUCT.into(), record).is_none(), "{name}");
    }
    let mut record = entry(app());
    record.publisher = "Unknown publisher".into();
    assert!(identify(app(), Hive::LocalMachine, KEY_WOW64_64KEY, PRODUCT.into(), record).is_none());
    let mut record = entry(app());
    record.windows_installer = 0;
    assert!(identify(app(), Hive::LocalMachine, KEY_WOW64_64KEY, PRODUCT.into(), record).is_none());
}

#[test]
fn accepts_only_a_valid_version_suffix_matching_display_version() {
    let mut record = entry(app());
    record.name = "PdfCraft 0.1.0".into();
    assert!(identify(app(), Hive::CurrentUser, KEY_WOW64_32KEY, PRODUCT.into(), record.clone()).is_some());
    record.version = Some("0.2.0".into());
    assert!(identify(app(), Hive::CurrentUser, KEY_WOW64_32KEY, PRODUCT.into(), record.clone()).is_none());
    for bad in ["0.1.0+&calc", "0.1.0;calc", "0.1.0 extra", "latest"] {
        record.name = format!("PdfCraft {bad}");
        record.version = Some(bad.into());
        assert!(identify(app(), Hive::CurrentUser, KEY_WOW64_32KEY, PRODUCT.into(), record.clone()).is_none());
    }
}

#[test]
fn requires_a_complete_product_guid_with_no_command_injection() {
    assert!(valid_product_code(PRODUCT));
    assert!(valid_product_code(&PRODUCT.to_ascii_lowercase()));
    for bad in [
        "12345678-1234-5678-90AB-CDEF12345678",
        "{12345678-1234-5678-90AB-CDEF1234567G}",
        "{12345678-1234-5678-90AB-CDEF12345678} /qn",
        "{12345678-1234-5678-90AB-CDEF12345678}&calc",
        r"..\{12345678-1234-5678-90AB-CDEF12345678}",
        "",
    ] {
        assert!(!valid_product_code(bad), "{bad}");
        assert!(identify(app(), Hive::LocalMachine, KEY_WOW64_64KEY, bad.into(), entry(app())).is_none());
    }
}

#[test]
fn builds_only_the_fixed_windows_installer_command() {
    let installed = install();
    let Identity::Windows(target) = &installed.identity else { panic!("fixture") };
    let system_msiexec = PathBuf::from(r"C:\Windows\System32\msiexec.exe");
    let command = uninstall_command(system_msiexec.clone(), target).expect("plan");
    assert_eq!(command.get_program(), system_msiexec.as_os_str());
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["/x", PRODUCT, "/passive", "/norestart"]);
    let mut invalid = target.clone();
    invalid.product_code.push_str(" /qn");
    assert!(uninstall_command(system_msiexec, &invalid).is_err());
}

#[test]
fn revalidates_the_exact_record_before_launch_and_reports_changes() {
    let installed = install();
    let launched = Cell::new(false);
    for current in [
        None,
        Some({
            let mut changed = entry(app());
            changed.version = Some("0.2.0".into());
            changed
        }),
    ] {
        let result = remove_checked(
            &installed,
            |_| Ok(current),
            |_| {
                launched.set(true);
                Ok(Some(0))
            },
        );
        assert!(matches!(result, Err(Error::Install(_))));
        assert!(!launched.get());
    }
    let result = remove_checked(
        &installed,
        |_| Ok(Some(entry(app()))),
        |_| {
            launched.set(true);
            Ok(Some(3010))
        },
    );
    assert!(result.expect("result").restart_required);
    assert!(launched.get());
}

#[test]
fn scan_errors_prevent_launch_and_process_errors_are_preserved() {
    let launched = Cell::new(false);
    let result = remove_checked(
        &install(),
        |_| Err(Error::Install("registry unavailable".into())),
        |_| {
            launched.set(true);
            Ok(Some(0))
        },
    );
    assert!(matches!(result, Err(Error::Install(_))));
    assert!(!launched.get());
    let result = remove_checked(&install(), |_| Ok(Some(entry(app()))), |_| Err(Error::Cancelled));
    assert_eq!(result, Err(Error::Cancelled));
}

#[test]
fn handles_success_restart_cancellation_busy_and_failed_removal() {
    for code in [0, 1605] {
        assert!(!removal_result(Some(code)).expect("success").restart_required);
    }
    for code in [1641, 3010] {
        assert!(removal_result(Some(code)).expect("restart").restart_required);
    }
    for code in [1602, 1223] {
        assert_eq!(removal_result(Some(code)), Err(Error::Cancelled));
    }
    assert!(matches!(removal_result(Some(1618)), Err(Error::Install(message)) if message.contains("another app")));
    assert!(matches!(removal_result(Some(1603)), Err(Error::Install(message)) if message.contains("1603")));
    assert!(matches!(removal_result(None), Err(Error::Install(_))));
}

#[test]
fn invalid_install_locations_are_never_used_as_paths() {
    for location in [r"..\PdfCraft", r"C:\Apps\..\Windows", ""] {
        let mut record = entry(app());
        record.location = Some(location.into());
        let detected = identify(app(), Hive::CurrentUser, KEY_WOW64_32KEY, PRODUCT.into(), record).expect("MSI identity independent of location");
        assert!(detected.location.is_none());
    }
}
