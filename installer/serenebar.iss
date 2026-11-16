; Inno Setup 6 script. Build the exe first, then:
;   iscc /DAppVersion=0.1.0 installer\serenebar.iss
; Installs per user with no admin prompt and writes the setup exe to target\installer\.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{8F3C2A51-6E4B-4C1D-9A7E-2B5D0C9E1F37}
AppName=serenebar
AppVersion={#AppVersion}
AppPublisher=serenebar
; under lowest privileges, {autopf} resolves to %LOCALAPPDATA%\Programs
DefaultDirName={autopf}\serenebar
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\target\installer
OutputBaseFilename=serenebar-setup-{#AppVersion}-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\serenebar.exe
SetupIconFile=..\icons\serenebar.ico
UninstallDisplayName=serenebar
; StopApp in [Code] closes the app itself, which gives it time to restore the Windows taskbar
CloseApplications=no

[Tasks]
Name: startup; Description: "Start serenebar when I sign in"

[Files]
Source: "..\target\release\serenebar.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\serenebar"; Filename: "{app}\serenebar.exe"

[Registry]
; same value the app's "Launch at startup" menu item writes
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "serenebar"; ValueData: """{app}\serenebar.exe"""; Tasks: startup; Flags: uninsdeletevalue
; remove it on uninstall even if it was turned on later from the app
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "serenebar"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\serenebar.exe"; Description: "Launch serenebar"; Flags: nowait postinstall

[Code]
// Sends a running serenebar WM_CLOSE, which makes it restore the taskbar and exit.
// After two seconds it force-kills whatever is left, so the exe can be replaced or removed.
procedure StopApp;
var
  R: Integer;
begin
  Exec(ExpandConstant('{cmd}'),
    '/c taskkill /IM serenebar.exe >nul 2>&1 & ping -n 3 127.0.0.1 >nul & taskkill /F /IM serenebar.exe >nul 2>&1',
    '', SW_HIDE, ewWaitUntilTerminated, R);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopApp;
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopApp;
end;
