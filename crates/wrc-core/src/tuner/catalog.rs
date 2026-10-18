//! The tweaks the Registry Tuner offers. Each is a fixed set of registry
//! changes, with what Windows has when the tweak is off. Tweaks that weaken
//! security (turning off UAC or SmartScreen) are deliberately absent.
//!
//! size-exception: one flat table of data; splitting it would scatter the
//! catalogue without separating any responsibility.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Appearance,
    Explorer,
    Taskbar,
    Performance,
    Privacy,
    Security,
    Updates,
}

/// What has to happen before a change shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Needs {
    Nothing,
    Explorer,
    SignOut,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Data {
    Dword(u32),
    Text(&'static str),
    /// Text with `%VARIABLES%` Windows expands (`REG_EXPAND_SZ`).
    Expand(&'static str),
}

/// One registry change. Keys are written `HKCU\…` or `HKLM\…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Set(&'static str, &'static str, Data),
    Remove(&'static str, &'static str),
    RemoveKey(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct Tweak {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub description: &'static str,
    pub needs: Needs,
    /// Only Windows 11 has the setting.
    pub windows11: bool,
    /// Only Windows 10 has the setting (Windows 11 removed it).
    pub windows10: bool,
    pub on: &'static [Change],
    pub off: &'static [Change],
}

use Change::{Remove, RemoveKey, Set};
use Data::{Dword, Expand, Text};

const ADVANCED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const EXPLORER: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer";
const DESKTOP: &str = r"HKCU\Control Panel\Desktop";
const CDM: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const CLASSIC_MENU: &str = r"HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";
const CLASSIC_MENU_SERVER: &str =
    r"HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32";
const EDGE: &str = r"HKLM\SOFTWARE\Policies\Microsoft\Edge";
const DUMPS: &str = r"HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps";
const AUTOCOMPLETE: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoComplete";
const CMD_HERE: &str = r"HKCU\Software\Classes\Directory\Background\shell\runas";
const CMD_HERE_COMMAND: &str = r"HKCU\Software\Classes\Directory\Background\shell\runas\command";
const OWN_FILE: &str = r"HKCU\Software\Classes\*\shell\runas";
const OWN_FILE_COMMAND: &str = r"HKCU\Software\Classes\*\shell\runas\command";
const OWN_FOLDER: &str = r"HKCU\Software\Classes\Directory\shell\runas";
const OWN_FOLDER_COMMAND: &str = r"HKCU\Software\Classes\Directory\shell\runas\command";
const PHOTO_VIEWER: &str =
    r"HKLM\SOFTWARE\Microsoft\Windows Photo Viewer\Capabilities\FileAssociations";
const USER_ACCOUNTS: &str = r"HKLM\SOFTWARE\Classes\CLSID\{98641F47-8C25-4936-BEE4-C2CE1298969D}";
const USER_ACCOUNTS_ICON: &str =
    r"HKLM\SOFTWARE\Classes\CLSID\{98641F47-8C25-4936-BEE4-C2CE1298969D}\DefaultIcon";
const USER_ACCOUNTS_OPEN: &str =
    r"HKLM\SOFTWARE\Classes\CLSID\{98641F47-8C25-4936-BEE4-C2CE1298969D}\Shell\Open\command";
const USER_ACCOUNTS_PANEL: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\ControlPanel\NameSpace\{98641F47-8C25-4936-BEE4-C2CE1298969D}";

const fn tweak(
    id: &'static str,
    group: Group,
    title: &'static str,
    description: &'static str,
    needs: Needs,
    on: &'static [Change],
    off: &'static [Change],
) -> Tweak {
    Tweak {
        id,
        group,
        title,
        description,
        needs,
        windows11: false,
        windows10: false,
        on,
        off,
    }
}

const fn win11(tweak: Tweak) -> Tweak {
    Tweak {
        windows11: true,
        ..tweak
    }
}

const fn win10(tweak: Tweak) -> Tweak {
    Tweak {
        windows10: true,
        ..tweak
    }
}

pub const TWEAKS: [Tweak; 51] = [
    win11(tweak(
        "classicContextMenu",
        Group::Appearance,
        "Classic right-click menu",
        "Show the full Windows 10 style right-click menu in File Explorer instead of the short one with \"Show more options\".",
        Needs::Explorer,
        &[Set(CLASSIC_MENU_SERVER, "", Text(""))],
        &[RemoveKey(CLASSIC_MENU)],
    )),
    tweak(
        "secondsInClock",
        Group::Taskbar,
        "Show seconds in the taskbar clock",
        "The clock shows seconds. Uses slightly more power.",
        Needs::Explorer,
        &[Set(ADVANCED, "ShowSecondsInSystemClock", Dword(1))],
        &[Remove(ADVANCED, "ShowSecondsInSystemClock")],
    ),
    win11(tweak(
        "taskbarLeft",
        Group::Taskbar,
        "Align taskbar icons to the left",
        "Start and the taskbar icons sit at the left, as in Windows 10, instead of the centre.",
        Needs::Explorer,
        &[Set(ADVANCED, "TaskbarAl", Dword(0))],
        &[Remove(ADVANCED, "TaskbarAl")],
    )),
    win11(tweak(
        "endTask",
        Group::Taskbar,
        "\"End task\" on taskbar right-click",
        "Right-clicking a taskbar button offers End task, to close a program that stopped responding.",
        Needs::Explorer,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings",
            "TaskbarEndTask",
            Dword(1),
        )],
        &[Remove(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings",
            "TaskbarEndTask",
        )],
    )),
    tweak(
        "noShortcutSuffix",
        Group::Explorer,
        "No \" - Shortcut\" on new shortcuts",
        "New shortcuts are named after what they open, without \" - Shortcut\" added.",
        Needs::Explorer,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\NamingTemplates",
            "ShortcutNameTemplate",
            Text("%s.lnk"),
        )],
        &[Remove(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\NamingTemplates",
            "ShortcutNameTemplate",
        )],
    ),
    tweak(
        "driveLettersFirst",
        Group::Explorer,
        "Drive letters before drive names",
        "This PC shows \"(C:) Windows\" rather than \"Windows (C:)\".",
        Needs::Explorer,
        &[Set(EXPLORER, "ShowDriveLettersFirst", Dword(4))],
        &[Remove(EXPLORER, "ShowDriveLettersFirst")],
    ),
    tweak(
        "showExtensions",
        Group::Explorer,
        "Show file name extensions",
        "File Explorer shows .exe, .txt and other extensions, which makes disguised files easier to spot.",
        Needs::Explorer,
        &[Set(ADVANCED, "HideFileExt", Dword(0))],
        &[Set(ADVANCED, "HideFileExt", Dword(1))],
    ),
    tweak(
        "showHidden",
        Group::Explorer,
        "Show hidden files and folders",
        "File Explorer lists files and folders marked hidden.",
        Needs::Explorer,
        &[Set(ADVANCED, "Hidden", Dword(1))],
        &[Set(ADVANCED, "Hidden", Dword(2))],
    ),
    tweak(
        "openThisPc",
        Group::Explorer,
        "Open File Explorer to This PC",
        "New File Explorer windows start at This PC (your drives) instead of Home.",
        Needs::Nothing,
        &[Set(ADVANCED, "LaunchTo", Dword(1))],
        &[Remove(ADVANCED, "LaunchTo")],
    ),
    tweak(
        "noInfoTips",
        Group::Explorer,
        "No pop-up descriptions on hover",
        "File Explorer stops showing a tooltip when the pointer rests on a file or folder.",
        Needs::Explorer,
        &[Set(ADVANCED, "ShowInfoTip", Dword(0))],
        &[Remove(ADVANCED, "ShowInfoTip")],
    ),
    tweak(
        "copyDetails",
        Group::Explorer,
        "Show details when copying and deleting",
        "The file progress window opens with its details (speed graph and file names) shown.",
        Needs::Nothing,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager",
            "EnthusiastMode",
            Dword(1),
        )],
        &[Remove(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager",
            "EnthusiastMode",
        )],
    ),
    tweak(
        "noNewAppAlert",
        Group::Explorer,
        "No \"You have new apps that can open this type of file\"",
        "Windows stops asking which app to use after a new app that opens a file type is installed.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\Explorer",
            "NoNewAppAlert",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\Explorer",
            "NoNewAppAlert",
        )],
    ),
    tweak(
        "noAutoplay",
        Group::Security,
        "Turn off AutoPlay",
        "Nothing starts by itself when a USB drive, memory card or disc is inserted.",
        Needs::Nothing,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers",
            "DisableAutoplay",
            Dword(1),
        )],
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers",
            "DisableAutoplay",
            Dword(0),
        )],
    ),
    tweak(
        "classicAltTab",
        Group::Appearance,
        "Classic Alt+Tab",
        "Alt+Tab shows small icons, as in Windows XP, instead of window previews.",
        Needs::Explorer,
        &[Set(EXPLORER, "AltTabSettings", Dword(1))],
        &[Remove(EXPLORER, "AltTabSettings")],
    ),
    tweak(
        "noAeroPeek",
        Group::Appearance,
        "Turn off Peek at desktop",
        "Resting the pointer on the far corner of the taskbar no longer makes windows transparent.",
        Needs::SignOut,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\DWM",
            "EnableAeroPeek",
            Dword(0),
        )],
        &[Set(
            r"HKCU\Software\Microsoft\Windows\DWM",
            "EnableAeroPeek",
            Dword(1),
        )],
    ),
    tweak(
        "versionOnDesktop",
        Group::Appearance,
        "Show the Windows version on the desktop",
        "The edition and build number appear in the bottom-right corner of the desktop.",
        Needs::SignOut,
        &[Set(DESKTOP, "PaintDesktopVersion", Dword(1))],
        &[Set(DESKTOP, "PaintDesktopVersion", Dword(0))],
    ),
    tweak(
        "verboseStopScreen",
        Group::Security,
        "Details on the blue error screen",
        "If Windows stops with an error, the screen shows technical details instead of only the sad face.",
        Needs::Restart,
        &[Set(
            r"HKLM\SYSTEM\CurrentControlSet\Control\CrashControl",
            "DisplayParameters",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SYSTEM\CurrentControlSet\Control\CrashControl",
            "DisplayParameters",
        )],
    ),
    tweak(
        "noLogonBlur",
        Group::Appearance,
        "No blur on the sign-in screen",
        "The sign-in screen shows its background picture sharp.",
        Needs::SignOut,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\System",
            "DisableAcrylicBackgroundOnLogon",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\System",
            "DisableAcrylicBackgroundOnLogon",
        )],
    ),
    tweak(
        "noWebSearch",
        Group::Taskbar,
        "No Bing results in Start search",
        "Searching from Start or the taskbar searches this PC only, not the web.",
        Needs::Explorer,
        &[Set(
            r"HKCU\Software\Policies\Microsoft\Windows\Explorer",
            "DisableSearchBoxSuggestions",
            Dword(1),
        )],
        &[Remove(
            r"HKCU\Software\Policies\Microsoft\Windows\Explorer",
            "DisableSearchBoxSuggestions",
        )],
    ),
    win11(tweak(
        "noWidgets",
        Group::Taskbar,
        "Turn off Widgets",
        "Removes the Widgets board and its taskbar button.",
        Needs::Explorer,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\Dsh",
            "AllowNewsAndInterests",
            Dword(0),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\Dsh",
            "AllowNewsAndInterests",
        )],
    )),
    tweak(
        "fasterMenus",
        Group::Performance,
        "Faster menus",
        "Submenus open after 100 ms instead of 400 ms.",
        Needs::SignOut,
        &[Set(DESKTOP, "MenuShowDelay", Text("100"))],
        &[Set(DESKTOP, "MenuShowDelay", Text("400"))],
    ),
    tweak(
        "fasterHover",
        Group::Performance,
        "Faster hover",
        "Tooltips and previews appear after 100 ms of hovering instead of 400 ms.",
        Needs::SignOut,
        &[Set(
            r"HKCU\Control Panel\Mouse",
            "MouseHoverTime",
            Text("100"),
        )],
        &[Set(
            r"HKCU\Control Panel\Mouse",
            "MouseHoverTime",
            Text("400"),
        )],
    ),
    tweak(
        "autoEndTasks",
        Group::Performance,
        "Close stuck apps at shutdown",
        "Shutting down or signing out closes programs that stop responding instead of waiting on the \"This app is preventing shutdown\" screen. Unsaved work in them is lost.",
        Needs::SignOut,
        &[Set(DESKTOP, "AutoEndTasks", Text("1"))],
        &[Remove(DESKTOP, "AutoEndTasks")],
    ),
    tweak(
        "iconCache",
        Group::Performance,
        "Larger icon cache",
        "Windows keeps up to 4,096 icons in memory, so folders with many file types open faster.",
        Needs::Restart,
        &[Set(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer",
            "Max Cached Icons",
            Text("4096"),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer",
            "Max Cached Icons",
        )],
    ),
    tweak(
        "noPowerThrottling",
        Group::Performance,
        "Turn off power throttling",
        "Programs in the background are no longer slowed down to save power. Uses more battery.",
        Needs::Restart,
        &[Set(
            r"HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling",
            "PowerThrottlingOff",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling",
            "PowerThrottlingOff",
        )],
    ),
    tweak(
        "noBackgroundApps",
        Group::Performance,
        "Stop Store apps running in the background",
        "Microsoft Store apps no longer run while closed. They may not show notifications.",
        Needs::Nothing,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications",
            "GlobalUserDisabled",
            Dword(1),
        )],
        &[Remove(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications",
            "GlobalUserDisabled",
        )],
    ),
    tweak(
        "noEdgeBackground",
        Group::Performance,
        "Stop Microsoft Edge running in the background",
        "Edge closes fully when its last window closes and does not preload at sign-in.",
        Needs::Nothing,
        &[
            Set(EDGE, "BackgroundModeEnabled", Dword(0)),
            Set(EDGE, "StartupBoostEnabled", Dword(0)),
        ],
        &[
            Remove(EDGE, "BackgroundModeEnabled"),
            Remove(EDGE, "StartupBoostEnabled"),
        ],
    ),
    tweak(
        "minimalTelemetry",
        Group::Privacy,
        "Send Microsoft the least diagnostic data",
        "Sets the diagnostic data policy to its lowest level. Home and Pro treat this as \"required diagnostic data only\".",
        Needs::Restart,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
            Dword(0),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
        )],
    ),
    tweak(
        "noAdvertisingId",
        Group::Privacy,
        "Turn off the advertising ID",
        "Apps can no longer use your advertising ID to show personalised ads.",
        Needs::Nothing,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            "Enabled",
            Dword(0),
        )],
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            "Enabled",
            Dword(1),
        )],
    ),
    tweak(
        "noSuggestions",
        Group::Privacy,
        "No suggested apps and promotions",
        "Stops Windows installing promoted apps by itself and showing suggestions in Start, Settings and File Explorer.",
        Needs::SignOut,
        &[
            Set(CDM, "SilentInstalledAppsEnabled", Dword(0)),
            Set(CDM, "SystemPaneSuggestionsEnabled", Dword(0)),
            Set(CDM, "SubscribedContent-338388Enabled", Dword(0)),
            Set(CDM, "SubscribedContent-338389Enabled", Dword(0)),
            Set(CDM, "SubscribedContent-338393Enabled", Dword(0)),
            Set(CDM, "SubscribedContent-353694Enabled", Dword(0)),
            Set(CDM, "SubscribedContent-353696Enabled", Dword(0)),
            Set(ADVANCED, "ShowSyncProviderNotifications", Dword(0)),
        ],
        &[
            Set(CDM, "SilentInstalledAppsEnabled", Dword(1)),
            Set(CDM, "SystemPaneSuggestionsEnabled", Dword(1)),
            Set(CDM, "SubscribedContent-338388Enabled", Dword(1)),
            Set(CDM, "SubscribedContent-338389Enabled", Dword(1)),
            Set(CDM, "SubscribedContent-338393Enabled", Dword(1)),
            Set(CDM, "SubscribedContent-353694Enabled", Dword(1)),
            Set(CDM, "SubscribedContent-353696Enabled", Dword(1)),
            Set(ADVANCED, "ShowSyncProviderNotifications", Dword(1)),
        ],
    ),
    tweak(
        "noLockScreenTips",
        Group::Privacy,
        "No tips on the lock screen",
        "The lock screen shows its picture without fun facts, tips and promotions.",
        Needs::SignOut,
        &[
            Set(CDM, "RotatingLockScreenOverlayEnabled", Dword(0)),
            Set(CDM, "SubscribedContent-338387Enabled", Dword(0)),
        ],
        &[
            Set(CDM, "RotatingLockScreenOverlayEnabled", Dword(1)),
            Set(CDM, "SubscribedContent-338387Enabled", Dword(1)),
        ],
    ),
    tweak(
        "cameraNotice",
        Group::Privacy,
        "Notify when the camera turns on or off",
        "Windows shows a notice each time a camera starts or stops, useful when the camera has no light.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SOFTWARE\Microsoft\OEM\Device\Capture",
            "NoPhysicalCameraLED",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Microsoft\OEM\Device\Capture",
            "NoPhysicalCameraLED",
        )],
    ),
    tweak(
        "blockUnwantedApps",
        Group::Security,
        "Block potentially unwanted apps",
        "Microsoft Defender blocks adware, bundled toolbars and similar unwanted programs.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows Defender",
            "PUAProtection",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows Defender",
            "PUAProtection",
        )],
    ),
    tweak(
        "usbReadOnly",
        Group::Security,
        "Make USB drives read-only",
        "Nothing can be written to USB storage. Takes effect the next time a drive is plugged in.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SYSTEM\CurrentControlSet\Control\StorageDevicePolicies",
            "WriteProtect",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SYSTEM\CurrentControlSet\Control\StorageDevicePolicies",
            "WriteProtect",
        )],
    ),
    tweak(
        "clearPageFile",
        Group::Security,
        "Clear the page file at shutdown",
        "Windows wipes its page file when shutting down, so memory contents do not stay on disk. Shutdown takes longer.",
        Needs::Restart,
        &[Set(
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
            "ClearPageFileAtShutdown",
            Dword(1),
        )],
        &[Set(
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
            "ClearPageFileAtShutdown",
            Dword(0),
        )],
    ),
    tweak(
        "crashDumps",
        Group::Security,
        "Keep crash dumps of programs",
        "When a program crashes, Windows saves a dump in %LOCALAPPDATA%\\CrashDumps (the last 10), for troubleshooting.",
        Needs::Nothing,
        &[Set(DUMPS, "DumpCount", Dword(10))],
        &[RemoveKey(DUMPS)],
    ),
    tweak(
        "noDriverUpdates",
        Group::Updates,
        "No driver updates from Windows Update",
        "Windows Update stops installing drivers; you update them yourself.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate",
            "ExcludeWUDriversInQualityUpdate",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate",
            "ExcludeWUDriversInQualityUpdate",
        )],
    ),
    tweak(
        "noStoreAutoUpdate",
        Group::Updates,
        "No automatic Store app updates",
        "Microsoft Store apps update only when you ask.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SOFTWARE\Policies\Microsoft\WindowsStore",
            "AutoDownload",
            Dword(2),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Policies\Microsoft\WindowsStore",
            "AutoDownload",
        )],
    ),
    tweak(
        "autoComplete",
        Group::Explorer,
        "Complete names as you type in File Explorer",
        "Typing in File Explorer's address bar or the Run box suggests names and completes the one you start.",
        Needs::Nothing,
        &[
            Set(AUTOCOMPLETE, "Append Completion", Text("yes")),
            Set(AUTOCOMPLETE, "AutoSuggest", Text("yes")),
        ],
        &[
            Remove(AUTOCOMPLETE, "Append Completion"),
            Remove(AUTOCOMPLETE, "AutoSuggest"),
        ],
    ),
    tweak(
        "doForAll",
        Group::Explorer,
        "Tick \"Do this for all current items\" by default",
        "When copying or moving meets files that already exist, the box applying your choice to all of them starts ticked.",
        Needs::Nothing,
        &[Set(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager",
            "ConfirmationCheckBoxDoForAll",
            Dword(1),
        )],
        &[Remove(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\OperationStatusManager",
            "ConfirmationCheckBoxDoForAll",
        )],
    ),
    tweak(
        "commandHereAdmin",
        Group::Explorer,
        "\"Command prompt here as administrator\" in folders",
        "Right-clicking inside a folder offers a command prompt there with administrator rights (Windows asks first).",
        Needs::Nothing,
        &[
            Set(CMD_HERE, "", Text("Command prompt here as administrator")),
            Set(CMD_HERE, "HasLUAShield", Text("")),
            Set(CMD_HERE_COMMAND, "", Text(r#"cmd.exe /s /k pushd "%V""#)),
        ],
        &[RemoveKey(CMD_HERE)],
    ),
    tweak(
        "takeOwnership",
        Group::Explorer,
        "\"Take ownership\" on files and folders",
        "Right-clicking a file or folder offers to make the Administrators group its owner with full control (Windows asks first). A folder's contents are included.",
        Needs::Nothing,
        &[
            Set(OWN_FILE, "", Text("Take ownership")),
            Set(OWN_FILE, "HasLUAShield", Text("")),
            Set(OWN_FILE, "NoWorkingDirectory", Text("")),
            Set(
                OWN_FILE_COMMAND,
                "",
                Text(r#"cmd.exe /c takeown /f "%1" && icacls "%1" /grant *S-1-5-32-544:F"#),
            ),
            Set(OWN_FOLDER, "", Text("Take ownership")),
            Set(OWN_FOLDER, "HasLUAShield", Text("")),
            Set(OWN_FOLDER, "NoWorkingDirectory", Text("")),
            Set(
                OWN_FOLDER_COMMAND,
                "",
                Text(
                    r#"cmd.exe /c takeown /f "%1" /r /d y && icacls "%1" /grant *S-1-5-32-544:F /t /c /q"#,
                ),
            ),
        ],
        &[RemoveKey(OWN_FILE), RemoveKey(OWN_FOLDER)],
    ),
    tweak(
        "photoViewer",
        Group::Explorer,
        "Windows Photo Viewer for pictures",
        "Offers the classic Windows Photo Viewer in \"Open with\" for JPEG, PNG, GIF and BMP pictures. Needs administrator rights.",
        Needs::Nothing,
        &[
            Set(PHOTO_VIEWER, ".bmp", Text("PhotoViewer.FileAssoc.Tiff")),
            Set(PHOTO_VIEWER, ".gif", Text("PhotoViewer.FileAssoc.Tiff")),
            Set(PHOTO_VIEWER, ".jpeg", Text("PhotoViewer.FileAssoc.Tiff")),
            Set(PHOTO_VIEWER, ".jpg", Text("PhotoViewer.FileAssoc.Tiff")),
            Set(PHOTO_VIEWER, ".png", Text("PhotoViewer.FileAssoc.Tiff")),
        ],
        &[
            Remove(PHOTO_VIEWER, ".bmp"),
            Remove(PHOTO_VIEWER, ".gif"),
            Remove(PHOTO_VIEWER, ".jpeg"),
            Remove(PHOTO_VIEWER, ".jpg"),
            Remove(PHOTO_VIEWER, ".png"),
        ],
    ),
    tweak(
        "classicUserAccounts",
        Group::Explorer,
        "Classic User Accounts in Control Panel",
        "Adds the classic User Accounts window (netplwiz), with automatic sign-in and group settings, to Control Panel. Needs administrator rights.",
        Needs::Nothing,
        &[
            Set(USER_ACCOUNTS, "", Text("Classic User Accounts")),
            Set(USER_ACCOUNTS, "InfoTip", Text("Manage user accounts")),
            Set(USER_ACCOUNTS, "System.ControlPanel.Category", Text("9")),
            Set(
                USER_ACCOUNTS_ICON,
                "",
                Expand(r"%SystemRoot%\System32\netplwiz.exe"),
            ),
            Set(USER_ACCOUNTS_OPEN, "", Text("Control Userpasswords2")),
            Set(USER_ACCOUNTS_PANEL, "", Text("Classic User Accounts")),
        ],
        &[RemoveKey(USER_ACCOUNTS), RemoveKey(USER_ACCOUNTS_PANEL)],
    ),
    tweak(
        "noShortcutArrows",
        Group::Appearance,
        "No arrows on shortcut icons",
        "Shortcuts show their program's icon without the small arrow. Needs administrator rights.",
        Needs::Explorer,
        &[Set(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Icons",
            "29",
            Expand(r"%SystemRoot%\System32\shell32.dll,-50"),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Icons",
            "29",
        )],
    ),
    tweak(
        "fasterThumbnails",
        Group::Performance,
        "Faster taskbar thumbnails",
        "Pointing at a taskbar button shows its windows' thumbnails at once instead of after a pause.",
        Needs::Explorer,
        &[Set(ADVANCED, "ExtendedUIHoverTime", Dword(1))],
        &[Remove(ADVANCED, "ExtendedUIHoverTime")],
    ),
    tweak(
        "fasterPeek",
        Group::Performance,
        "Faster Peek at desktop",
        "Resting the pointer on the far corner of the taskbar shows the desktop almost at once instead of after a pause.",
        Needs::Explorer,
        &[Set(ADVANCED, "DesktopLivePreviewHoverTime", Dword(100))],
        &[Remove(ADVANCED, "DesktopLivePreviewHoverTime")],
    ),
    win10(tweak(
        "classicVolume",
        Group::Appearance,
        "Classic volume control",
        "Clicking the speaker icon opens the old volume slider instead of the modern one. Windows 10 only; needs administrator rights.",
        Needs::Explorer,
        &[Set(
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\MTCUVC",
            "EnableMtcUvc",
            Dword(0),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\MTCUVC",
            "EnableMtcUvc",
        )],
    )),
    win10(tweak(
        "classicBattery",
        Group::Appearance,
        "Classic battery flyout",
        "Clicking the battery icon opens the old battery meter and power plan list. Windows 10 only; needs administrator rights.",
        Needs::Explorer,
        &[Set(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\ImmersiveShell",
            "UseWin32BatteryFlyout",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\ImmersiveShell",
            "UseWin32BatteryFlyout",
        )],
    )),
    win10(tweak(
        "noQuickActions",
        Group::Taskbar,
        "Action Center for notifications only",
        "Hides the quick action buttons at the bottom of the Action Center. Windows 10 only; needs administrator rights.",
        Needs::Explorer,
        &[Set(
            r"HKLM\SOFTWARE\Microsoft\Shell\ActionCenter\Quick Actions",
            "PinnedQuickActionSlotCount",
            Dword(0),
        )],
        &[Remove(
            r"HKLM\SOFTWARE\Microsoft\Shell\ActionCenter\Quick Actions",
            "PinnedQuickActionSlotCount",
        )],
    )),
    win10(tweak(
        "upgradeUnsupported",
        Group::Updates,
        "Allow the Windows 11 upgrade on unsupported hardware",
        "Lets Windows 11 Setup upgrade a PC whose processor or TPM is not on Microsoft's list (the setting Microsoft documents; such PCs get no support from it). Windows 10 only; needs administrator rights.",
        Needs::Nothing,
        &[Set(
            r"HKLM\SYSTEM\Setup\MoSetup",
            "AllowUpgradesWithUnsupportedTPMOrCPU",
            Dword(1),
        )],
        &[Remove(
            r"HKLM\SYSTEM\Setup\MoSetup",
            "AllowUpgradesWithUnsupportedTPMOrCPU",
        )],
    )),
];
