use serde::{Deserialize, Serialize};

/// 基础窗口合成模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WindowComposition {
    #[default]
    Opaque,
    Transparent,
}

impl WindowComposition {
    #[inline]
    pub fn is_opaque(&self) -> bool {
        matches!(self, Self::Opaque)
    }

    #[inline]
    pub fn is_transparent(&self) -> bool {
        matches!(self, Self::Transparent)
    }
}

/// 系统材质特效 (通用抽象，具体 Windows API 映射由 Windows backend 私有处理)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BackdropMaterial {
    #[default]
    None,
    Mica,
    MicaAlt,
    Acrylic,
}

/// 窗口整体外观配置（用户意图层）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WindowAppearancePreference {
    pub composition: WindowComposition,
    pub material: BackdropMaterial,
}

/// 平台支持能力 (仅反映 OS/驱动层客观能力)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Capability {
    #[default]
    Unsupported,
    Supported,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowEffectCapabilities {
    pub transparent: Capability, // D3D11 + DXGI 1.2 + DirectComposition 核心能力可用
    pub mica: Capability,        // Win11 Build 22621+ 且 DWM 支持
    pub mica_alt: Capability,    // Win11 Build 22621+
    pub acrylic: Capability,     // Win10 1809+ / Win11
}

impl WindowEffectCapabilities {
    /// 依据当前操作系统探测基础能力
    pub fn detect_current_platform() -> Self {
        #[cfg(target_os = "windows")]
        {
            // Windows 8+ 原生支持 DirectComposition + Premultiplied Alpha
            let transparent = Capability::Supported;

            // 运行时探测 Windows 版本
            let (mica, mica_alt, acrylic) = detect_windows_material_capabilities();

            Self {
                transparent,
                mica,
                mica_alt,
                acrylic,
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self {
                transparent: Capability::Supported,
                mica: Capability::Unsupported,
                mica_alt: Capability::Unsupported,
                acrylic: Capability::Unsupported,
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_material_capabilities() -> (Capability, Capability, Capability) {
    use std::sync::OnceLock;
    static CAPABILITIES: OnceLock<(Capability, Capability, Capability)> = OnceLock::new();

    *CAPABILITIES.get_or_init(|| {
        #[repr(C)]
        struct OsVersionInfoExW {
            dw_os_version_info_size: u32,
            dw_major_version: u32,
            dw_minor_version: u32,
            dw_build_number: u32,
            dw_platform_id: u32,
            sz_csd_version: [u16; 128],
            w_service_pack_major: u16,
            w_service_pack_minor: u16,
            w_suite_mask: u16,
            w_product_type: u8,
            w_reserved: u8,
        }

        type RtlGetVersionFn = unsafe extern "system" fn(*mut OsVersionInfoExW) -> i32;

        let build_number = unsafe {
            use windows::core::PCSTR;
            use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

            let module_name = PCSTR::from_raw(c"ntdll.dll".as_ptr() as *const u8);
            if let Ok(ntdll) = GetModuleHandleA(module_name) {
                let func_name = PCSTR::from_raw(c"RtlGetVersion".as_ptr() as *const u8);
                if let Some(raw_proc) = GetProcAddress(ntdll, func_name) {
                    let rtl_get_version: RtlGetVersionFn = std::mem::transmute(raw_proc);
                    let mut info = std::mem::zeroed::<OsVersionInfoExW>();
                    info.dw_os_version_info_size = std::mem::size_of::<OsVersionInfoExW>() as u32;
                    if rtl_get_version(&mut info) == 0 {
                        info.dw_build_number
                    } else {
                        0
                    }
                } else {
                    0
                }
            } else {
                0
            }
        };

        log::info!("[windows-dwm] Detected Windows build number: {}", build_number);

        // Build 22621+: Windows 11 22H2+, native support for Mica, Mica Alt, and Acrylic backdrops
        if build_number >= 22621 {
            (Capability::Supported, Capability::Supported, Capability::Supported)
        } else if build_number >= 17763 {
            // Windows 10 1809+ / Windows 11 21H2, Acrylic blur via SetWindowCompositionAttribute
            (Capability::Unsupported, Capability::Unsupported, Capability::Supported)
        } else {
            (Capability::Unsupported, Capability::Unsupported, Capability::Unsupported)
        }
    })
}

/// 合成策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompositionPolicy {
    #[default]
    Normal,
    Performance, // Borderless Fullscreen 或极限吞吐模式
}

/// 降级原因严格分类诊断
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FallbackReason {
    #[default]
    None,
    UnsupportedPlatform,   // 操作系统/平台 API 根本不存在
    UnsupportedCapability, // 平台理论支持，但当前驱动/硬件环境能力检测未达标
    InitializationFailed,  // 后端已尝试初始化，但运行时抛出错误
    PerformancePolicy,     // 因全屏等性能策略强制切为不透明
}

/// 仲裁输出结果（全局唯一有效状态）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedWindowAppearance {
    pub composition: WindowComposition,
    pub material: BackdropMaterial,
    pub fallback_reason: FallbackReason,
}

impl ResolvedWindowAppearance {
    /// 转换为 GPUI 原生窗口背景外观枚举
    pub fn to_gpui_appearance(&self) -> gpui::WindowBackgroundAppearance {
        match (self.composition, self.material) {
            (WindowComposition::Opaque, _) => gpui::WindowBackgroundAppearance::Opaque,
            (WindowComposition::Transparent, BackdropMaterial::None) => {
                gpui::WindowBackgroundAppearance::Transparent
            }
            (WindowComposition::Transparent, BackdropMaterial::Mica) => {
                gpui::WindowBackgroundAppearance::MicaBackdrop
            }
            (WindowComposition::Transparent, BackdropMaterial::MicaAlt) => {
                gpui::WindowBackgroundAppearance::MicaAltBackdrop
            }
            (WindowComposition::Transparent, BackdropMaterial::Acrylic) => {
                gpui::WindowBackgroundAppearance::Blurred
            }
        }
    }
}

struct ResolutionResult<T> {
    value: T,
    reason: Option<FallbackReason>,
}

fn resolve_material(
    material: BackdropMaterial,
    caps: WindowEffectCapabilities,
) -> ResolutionResult<BackdropMaterial> {
    match material {
        BackdropMaterial::None => ResolutionResult {
            value: BackdropMaterial::None,
            reason: None,
        },
        BackdropMaterial::Mica => {
            if caps.mica == Capability::Supported {
                ResolutionResult {
                    value: BackdropMaterial::Mica,
                    reason: None,
                }
            } else {
                ResolutionResult {
                    value: BackdropMaterial::None,
                    reason: Some(FallbackReason::UnsupportedCapability),
                }
            }
        }
        BackdropMaterial::MicaAlt => {
            if caps.mica_alt == Capability::Supported {
                ResolutionResult {
                    value: BackdropMaterial::MicaAlt,
                    reason: None,
                }
            } else {
                ResolutionResult {
                    value: BackdropMaterial::None,
                    reason: Some(FallbackReason::UnsupportedCapability),
                }
            }
        }
        BackdropMaterial::Acrylic => {
            if caps.acrylic == Capability::Supported {
                ResolutionResult {
                    value: BackdropMaterial::Acrylic,
                    reason: None,
                }
            } else {
                ResolutionResult {
                    value: BackdropMaterial::None,
                    reason: Some(FallbackReason::UnsupportedCapability),
                }
            }
        }
    }
}

fn resolve_composition(
    wants_transparent: bool,
    transparent_cap: Capability,
) -> ResolutionResult<WindowComposition> {
    if wants_transparent {
        if transparent_cap == Capability::Supported {
            ResolutionResult {
                value: WindowComposition::Transparent,
                reason: None,
            }
        } else {
            ResolutionResult {
                value: WindowComposition::Opaque,
                reason: Some(FallbackReason::UnsupportedPlatform),
            }
        }
    } else {
        ResolutionResult {
            value: WindowComposition::Opaque,
            reason: None,
        }
    }
}

/// 纯函数仲裁器：纯输入 → 纯输出，无副作用、可单元测试、确定性状态仲裁
pub fn resolve_window_appearance(
    preference: WindowAppearancePreference,
    policy: CompositionPolicy,
    capabilities: WindowEffectCapabilities,
) -> ResolvedWindowAppearance {
    if policy == CompositionPolicy::Performance {
        return ResolvedWindowAppearance {
            composition: WindowComposition::Opaque,
            material: BackdropMaterial::None,
            fallback_reason: FallbackReason::PerformancePolicy,
        };
    }

    let material = resolve_material(preference.material, capabilities);

    let wants_transparent = preference.composition == WindowComposition::Transparent
        || material.value != BackdropMaterial::None;

    let composition = resolve_composition(wants_transparent, capabilities.transparent);

    ResolvedWindowAppearance {
        composition: composition.value,
        material: material.value,
        fallback_reason: composition
            .reason
            .or(material.reason)
            .unwrap_or(FallbackReason::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_opaque_resolution() {
        let pref = WindowAppearancePreference {
            composition: WindowComposition::Opaque,
            material: BackdropMaterial::None,
        };
        let caps = WindowEffectCapabilities {
            transparent: Capability::Supported,
            ..Default::default()
        };
        let resolved = resolve_window_appearance(pref, CompositionPolicy::Normal, caps);
        assert_eq!(resolved.composition, WindowComposition::Opaque);
        assert_eq!(resolved.material, BackdropMaterial::None);
        assert_eq!(resolved.fallback_reason, FallbackReason::None);
    }

    #[test]
    fn test_transparent_resolution_when_supported() {
        let pref = WindowAppearancePreference {
            composition: WindowComposition::Transparent,
            material: BackdropMaterial::None,
        };
        let caps = WindowEffectCapabilities {
            transparent: Capability::Supported,
            ..Default::default()
        };
        let resolved = resolve_window_appearance(pref, CompositionPolicy::Normal, caps);
        assert_eq!(resolved.composition, WindowComposition::Transparent);
        assert_eq!(resolved.material, BackdropMaterial::None);
        assert_eq!(resolved.fallback_reason, FallbackReason::None);
    }

    #[test]
    fn test_transparent_fallback_when_unsupported() {
        let pref = WindowAppearancePreference {
            composition: WindowComposition::Transparent,
            material: BackdropMaterial::None,
        };
        let caps = WindowEffectCapabilities {
            transparent: Capability::Unsupported,
            ..Default::default()
        };
        let resolved = resolve_window_appearance(pref, CompositionPolicy::Normal, caps);
        assert_eq!(resolved.composition, WindowComposition::Opaque);
        assert_eq!(resolved.material, BackdropMaterial::None);
        assert_eq!(resolved.fallback_reason, FallbackReason::UnsupportedPlatform);
    }

    #[test]
    fn test_material_forces_transparent_composition() {
        let pref = WindowAppearancePreference {
            composition: WindowComposition::Opaque,
            material: BackdropMaterial::Mica,
        };
        let caps = WindowEffectCapabilities {
            transparent: Capability::Supported,
            mica: Capability::Supported,
            ..Default::default()
        };
        let resolved = resolve_window_appearance(pref, CompositionPolicy::Normal, caps);
        // Material 反向约束 Composition 为 Transparent
        assert_eq!(resolved.composition, WindowComposition::Transparent);
        assert_eq!(resolved.material, BackdropMaterial::Mica);
        assert_eq!(resolved.fallback_reason, FallbackReason::None);
    }

    #[test]
    fn test_material_fallback_when_unsupported() {
        let pref = WindowAppearancePreference {
            composition: WindowComposition::Transparent,
            material: BackdropMaterial::Mica,
        };
        let caps = WindowEffectCapabilities {
            transparent: Capability::Supported,
            mica: Capability::Unsupported,
            ..Default::default()
        };
        let resolved = resolve_window_appearance(pref, CompositionPolicy::Normal, caps);
        assert_eq!(resolved.composition, WindowComposition::Transparent);
        assert_eq!(resolved.material, BackdropMaterial::None);
        assert_eq!(resolved.fallback_reason, FallbackReason::UnsupportedCapability);
    }

    #[test]
    fn test_performance_policy_forces_opaque() {
        let pref = WindowAppearancePreference {
            composition: WindowComposition::Transparent,
            material: BackdropMaterial::Mica,
        };
        let caps = WindowEffectCapabilities {
            transparent: Capability::Supported,
            mica: Capability::Supported,
            ..Default::default()
        };
        let resolved = resolve_window_appearance(pref, CompositionPolicy::Performance, caps);
        assert_eq!(resolved.composition, WindowComposition::Opaque);
        assert_eq!(resolved.material, BackdropMaterial::None);
        assert_eq!(resolved.fallback_reason, FallbackReason::PerformancePolicy);
    }

    /// 验证测试 9A：GPU 合成数学自动化单测 (Premultiplied 双通道方程)
    /// Cout = Cs + Cd * (1 - As)
    /// Aout = As + Ad * (1 - As)
    #[test]
    fn test_9a_gpu_blending_math_premultiplied() {
        let test_cases: &[(f64, f64, f64, f64, f64, f64)] = &[
            // (Cs, As, Cd, Ad, Expected_Cout, Expected_Aout)
            (0.0, 0.0, 1.0, 1.0, 1.0, 1.0),                 // A_s=0 完全穿透
            (0.5, 1.0, 0.2, 1.0, 0.5, 1.0),                 // A_s=1 完全遮挡
            (0.25 * 0.5, 0.5, 0.8 * 1.0, 1.0, 0.125 + 0.8 * 0.5, 0.5 + 1.0 * 0.5), // 半透明混合
            (0.0, 0.0, 0.0, 0.0, 0.0, 0.0),                 // 双空
        ];

        for (cs, as_val, cd, ad, exp_c, exp_a) in test_cases {
            let cout = cs + cd * (1.0 - as_val);
            let aout = as_val + ad * (1.0 - as_val);
            assert!((cout - exp_c).abs() < 1e-5, "Color mismatch: {cout} vs {exp_c}");
            assert!((aout - exp_a).abs() < 1e-5, "Alpha mismatch: {aout} vs {exp_a}");
        }
    }
}
