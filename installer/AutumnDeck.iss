; AutumnDeck Production Installer Script (Inno Setup 6/7)
; Builds a zero-UAC per-user installer by default (with optional admin override),
; self-heals AutumnXamlTap.dll ACL permissions, and guarantees clean taskbar
; restoration + startup task cleanup on uninstall.

#define MyAppName "AutumnDeck"
#define MyAppVersion "2.0.0"
#define MyAppPublisher "AutumnDeck"
#define MyAppExeName "AutumnDeck.exe"
#define MyAppDesc "Native Windows 11 Taskbar Styler, Interactive PBR Docks, Modular Desktop Widgets & Live Wallpaper Suite"

[Setup]
AppId={{C3A9E27D-6B41-48F5-9E12-7D3A5B1F804A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppComments={#MyAppDesc}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
OutputDir=..\dist
OutputBaseFilename=AutumnDeck-Setup-{#MyAppVersion}
SetupIconFile=..\assets\AutumnDeck.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern dynamic
CloseApplications=no
RestartIfNeededByRun=no
VersionInfoVersion=2.0.0.0
VersionInfoProductName={#MyAppName}
VersionInfoDescription={#MyAppName} Setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start {#MyAppName} automatically when Windows starts"; GroupDescription: "Startup options:"
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "..\AutumnDeck.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\AutumnGuard.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\AutumnXamlTap.dll"; DestDir: "{app}"; Flags: ignoreversion restartreplace uninsrestartdelete
Source: "..\assets\AutumnDeck.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\Autumn_Sanctuary_Starter.mp4"; DestDir: "{app}\Wallpapers"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\AutumnDeck.ico"
Name: "{group}\{#MyAppName} Settings"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--settings"; IconFilename: "{app}\AutumnDeck.ico"
Name: "{group}\Restore Default Windows Taskbar"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--restore"; IconFilename: "{app}\AutumnDeck.ico"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\AutumnDeck.ico"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "AutumnDeck"; ValueData: """{app}\{#MyAppExeName}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
; Repair ALL APPLICATION PACKAGES (S-1-15-2-1) RX ACL using installer permissions
Filename: "{app}\{#MyAppExeName}"; Parameters: "--repair-acl"; Flags: runhidden waituntilterminated
; Optionally launch AutumnDeck after install
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName} now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Cleanly stop running instance, restore native Windows taskbar/workarea, and remove logon task
Filename: "{app}\{#MyAppExeName}"; Parameters: "--uninstall-cleanup"; RunOnceId: "AutumnDeckCleanup"; Flags: runhidden waituntilterminated

[UninstallDelete]
Type: filesandordirs; Name: "{app}\Themes"
Type: files; Name: "{app}\AutumnXamlTap.old.dll"
Type: files; Name: "{app}\AutumnDeck.old.exe"

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ExePath: String;
  TapDll: String;
  OldDll: String;
  ResultCode: Integer;
begin
  Result := '';
  ExePath := ExpandConstant('{app}\{#MyAppExeName}');
  if FileExists(ExePath) then
  begin
    // Ask any existing installed instance to exit cleanly & restore before updating files
    Exec(ExePath, '--quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Sleep(700);
  end;

  // If Explorer currently holds AutumnXamlTap.dll open, rename it out of the way so
  // the new DLL can install cleanly without forcing a reboot.
  TapDll := ExpandConstant('{app}\AutumnXamlTap.dll');
  OldDll := ExpandConstant('{app}\AutumnXamlTap.old.dll');
  if FileExists(TapDll) then
  begin
    DeleteFile(OldDll);
    RenameFile(TapDll, OldDll);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  AppDataDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    AppDataDir := ExpandConstant('{userappdata}\AutumnDeck');
    if DirExists(AppDataDir) and (not UninstallSilent) then
    begin
      if MsgBox(
        'Do you also want to remove your saved AutumnDeck themes and configuration?' + #13#10 + #13#10 +
        'Folder: ' + AppDataDir + #13#10 +
        '(Choose No if you plan to reinstall later.)',
        mbConfirmation,
        MB_YESNO or MB_DEFBUTTON2
      ) = IDYES then
      begin
        DelTree(AppDataDir, True, True, True);
      end;
    end;
  end;
end;
