; Windmolen installer, built with Inno Setup 6 by .github/workflows/release.yml:
;   ISCC /DVersion=0.1.0 /DSource=dist\windmolen.exe /DIcon=path\to\windmolen.ico /Odist installer\windmolen.iss
;
; Installs for the current user only (no administrator rights) into
; %LOCALAPPDATA%\Programs\Windmolen, adds a Start menu entry and an entry in
; Settings > Apps. The program registers its image formats itself
; (--register), from the codecs Windows has, and removes them on uninstall.
; The app updates itself by running a newer installer with
;   /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /UPDATE=1 [/OPEN=<image>]

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\target\release\windmolen.exe"
#endif

[Setup]
; Never change: Windows tells installations apart by it.
AppId={{C5FEA1CD-40E6-44C8-AE36-05444E6CACD1}
AppName=Windmolen
AppVersion={#Version}
AppVerName=Windmolen {#Version}
AppPublisher=professional-cynic
AppPublisherURL=https://github.com/professional-cynic/windmolen
AppSupportURL=https://github.com/professional-cynic/windmolen/issues
AppUpdatesURL=https://github.com/professional-cynic/windmolen/releases
VersionInfoVersion={#Version}
DefaultDirName={autopf}\Windmolen
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1607: the program restricts its DLL search to System32.
MinVersion=10.0.14393
ChangesAssociations=yes
WizardStyle=modern
#ifdef Icon
SetupIconFile={#Icon}
#endif
UninstallDisplayIcon={app}\windmolen.exe
UninstallDisplayName=Windmolen
LicenseFile=..\LICENSE
OutputBaseFilename=Windmolen-Setup
Compression=lzma2/max
SolidCompression=yes

[Files]
Source: "{#Source}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Windmolen"; Filename: "{app}\windmolen.exe"; Comment: "A lean, fast photo viewer"

[Run]
Filename: "{app}\windmolen.exe"; Parameters: "--register"; StatusMsg: "Registering image formats…"; Flags: runhidden waituntilterminated
Filename: "{app}\windmolen.exe"; Description: "Start Windmolen"; Flags: postinstall nowait skipifsilent
Filename: "ms-settings:defaultapps?registeredAppUser=Windmolen"; Description: "Open Default apps in Settings"; Flags: postinstall shellexec nowait skipifsilent
; An update run by the app (/UPDATE=1): start it again, with the image it showed.
Filename: "{app}\windmolen.exe"; Parameters: "{code:OpenParam}"; Check: IsUpdate; Flags: nowait

[UninstallRun]
Filename: "{app}\windmolen.exe"; Parameters: "--unregister"; Flags: runhidden waituntilterminated; RunOnceId: "Unregister"

[UninstallDelete]
; Left by the built-in updater when it replaced a running copy.
Type: files; Name: "{app}\windmolen.old-*.exe"
Type: files; Name: "{app}\.windmolen-*.tmp"

[Code]
function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

function OpenParam(Param: String): String;
begin
  Result := ExpandConstant('{param:OPEN|}');
  if Result <> '' then
    Result := AddQuotes(Result);
end;

[Messages]
FinishedLabel=Windmolen is installed.%n%nTo open images with it by default: in Settings › Apps › Default apps, choose Windmolen and select Set default. On Windows 10, choose Windmolen under Photo viewer instead.
