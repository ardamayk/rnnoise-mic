; Inno Setup script for the Windows installer.
; Build: iscc installer\rnnoise-mic.iss  (optionally /DAppVersion=1.2.3 /DBinDir=path)
; Installs per user (no admin prompt) and registers in Settings > Apps > Installed apps.

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
#endif

#define AppName "RNNoise Mic"
#define AppExe "rnnoise-mic.exe"
; Must match autostart::NAME in src/autostart.rs.
#define RunValue "rnnoise-mic"

[Setup]
AppId={{8C3F6D2E-5B1A-4E7C-9A44-2F6B1D0E7A31}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Arda Mike
AppPublisherURL=https://github.com/ardamayk/rnnoise-mic
AppSupportURL=https://github.com/ardamayk/rnnoise-mic/issues
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=rnnoise-mic-setup-{#AppVersion}
SetupIconFile=..\assets\rnnoise-mic.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
InfoAfterFile=after-install.txt
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
; Upgrades: close the running tray app (Restart Manager) and start it again afterwards.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"

[CustomMessages]
english.AutoStart=Start {#AppName} when I sign in to Windows
turkish.AutoStart={#AppName} Windows oturumu açılınca başlasın
english.LaunchNow=Launch {#AppName} now
turkish.LaunchNow={#AppName} şimdi başlatılsın

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Same value the tray menu's "Start at login" writes, so both stay in sync.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
  ValueName: "{#RunValue}"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchNow}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "StopApp"

[Code]
// Remove "start at login" whether it was set by the installer or from the tray menu.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#RunValue}');
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run', '{#RunValue}');
  end;
end;
