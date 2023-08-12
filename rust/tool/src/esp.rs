use std::array::IntoIter;
use std::path::{Path, PathBuf};

/// Paths to the boot files that are not specific to a generation.
pub struct EspPaths {
    pub esp: PathBuf,
    pub efi: PathBuf,
    pub nixos: PathBuf,
    pub linux: PathBuf,
    pub efi_fallback_dir: PathBuf,
    pub efi_fallback: PathBuf,
    pub systemd: PathBuf,
    pub systemd_boot: PathBuf,
    pub loader: PathBuf,
    pub systemd_boot_loader_config: PathBuf,
}

impl EspPaths {
    pub fn new(esp: impl AsRef<Path>) -> Self {
        let esp = esp.as_ref();
        let efi = esp.join("EFI");
        let efi_nixos = efi.join("nixos");
        let efi_linux = efi.join("Linux");
        let efi_systemd = efi.join("systemd");
        let efi_efi_fallback_dir = efi.join("BOOT");
        let loader = esp.join("loader");
        let systemd_boot_loader_config = loader.join("loader.conf");

        Self {
            esp: esp.to_path_buf(),
            efi,
            nixos: efi_nixos,
            linux: efi_linux,
            efi_fallback_dir: efi_efi_fallback_dir.clone(),
            efi_fallback: efi_efi_fallback_dir.join("BOOTX64.EFI"),
            systemd: efi_systemd.clone(),
            systemd_boot: efi_systemd.join("systemd-bootx64.efi"),
            loader,
            systemd_boot_loader_config,
        }
    }

    /// Return the used file paths to store as garbage collection roots.
    pub fn to_iter(&self) -> IntoIter<&PathBuf, 10> {
        [
            &self.esp,
            &self.efi,
            &self.nixos,
            &self.linux,
            &self.efi_fallback_dir,
            &self.efi_fallback,
            &self.systemd,
            &self.systemd_boot,
            &self.loader,
            &self.systemd_boot_loader_config,
        ]
        .into_iter()
    }
}
