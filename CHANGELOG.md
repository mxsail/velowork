# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive Git workflow and version release specification (`rules/07-Git工作流与版本发布规范.md`).

### Fixed
- Fixed custom title bar and window caption buttons not rendering on Windows 11 under client-side decoration mode.
- Fixed taskbar right-click "Close window" and system exit unresponsiveness by distinguishing OS close from titlebar close, unminimizing/activating window before showing confirmation dialog, and defaulting close behavior to Exit.

## [0.1.0-beta.1] - 2026-09-10

### Added
- Initial public beta release of Velowork.
- Cross-platform GPU-accelerated terminal and workspace application built with GPUI.
- Support for local shells and remote SSH session management with credential encryption.
- Multi-pane terminal splitting (horizontal/vertical) and tab management.
- Theme system and configurable keyboard shortcuts.
- Automated CI matrix release packaging for Linux (`.AppImage`, `.deb`), macOS (`.dmg`, `.zip` for ARM64 & Intel), and Windows (`.msi`, `.zip`).
