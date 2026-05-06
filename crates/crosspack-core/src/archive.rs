#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    Zip,
    TarGz,
    TarXz,
    TarZst,
    Bin,
    Msi,
    Dmg,
    AppImage,
    Exe,
    Pkg,
    Msix,
    Appx,
}

impl ArchiveType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarXz => "tar.xz",
            Self::TarZst => "tar.zst",
            Self::Bin => "bin",
            Self::Msi => "msi",
            Self::Dmg => "dmg",
            Self::AppImage => "appimage",
            Self::Exe => "exe",
            Self::Pkg => "pkg",
            Self::Msix => "msix",
            Self::Appx => "appx",
        }
    }

    pub fn cache_extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarXz => "tar.xz",
            Self::TarZst => "tar.zst",
            Self::Bin => "bin",
            Self::Msi => "msi",
            Self::Dmg => "dmg",
            Self::AppImage => "appimage",
            Self::Exe => "exe",
            Self::Pkg => "pkg",
            Self::Msix => "msix",
            Self::Appx => "appx",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "zip" => Some(Self::Zip),
            "tar.gz" | "tgz" => Some(Self::TarGz),
            "tar.xz" | "txz" => Some(Self::TarXz),
            "tar.zst" | "tzst" => Some(Self::TarZst),
            "bin" => Some(Self::Bin),
            "msi" => Some(Self::Msi),
            "dmg" => Some(Self::Dmg),
            "appimage" => Some(Self::AppImage),
            "exe" => Some(Self::Exe),
            "pkg" => Some(Self::Pkg),
            "msix" => Some(Self::Msix),
            "appx" => Some(Self::Appx),
            _ => None,
        }
    }

    pub fn supports_source_build(self) -> bool {
        matches!(self, Self::Zip | Self::TarGz | Self::TarXz | Self::TarZst)
    }

    pub fn infer_from_url(url: &str) -> Option<Self> {
        let without_fragment = url.split('#').next().unwrap_or(url);
        let without_query = without_fragment
            .split('?')
            .next()
            .unwrap_or(without_fragment);
        let normalized = without_query.to_ascii_lowercase();

        if normalized.ends_with(".zip") {
            return Some(Self::Zip);
        }
        if normalized.ends_with(".tar.gz") || normalized.ends_with(".tgz") {
            return Some(Self::TarGz);
        }
        if normalized.ends_with(".tar.xz") || normalized.ends_with(".txz") {
            return Some(Self::TarXz);
        }
        if normalized.ends_with(".tar.zst") || normalized.ends_with(".tzst") {
            return Some(Self::TarZst);
        }
        if normalized.ends_with(".bin") {
            return Some(Self::Bin);
        }
        if normalized.ends_with(".msi") {
            return Some(Self::Msi);
        }
        if normalized.ends_with(".dmg") {
            return Some(Self::Dmg);
        }
        if normalized.ends_with(".appimage") {
            return Some(Self::AppImage);
        }
        if normalized.ends_with(".exe") {
            return Some(Self::Exe);
        }
        if normalized.ends_with(".pkg") {
            return Some(Self::Pkg);
        }
        if normalized.ends_with(".msix") {
            return Some(Self::Msix);
        }
        if normalized.ends_with(".appx") {
            return Some(Self::Appx);
        }

        let file_name = normalized.rsplit('/').next().unwrap_or("");
        if !file_name.is_empty() && !file_name.contains('.') {
            return Some(Self::Bin);
        }

        None
    }
}
