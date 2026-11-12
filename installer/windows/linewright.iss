; The Windows installer: Linewright in Program Files (or the user's own programs folder, without asking for an
; administrator), a Start menu entry, a desktop icon if wanted, DXF and DWG files opening in it if wanted, and an
; entry in Installed apps to remove it again.
;
; Built by the release workflow with Inno Setup:
;   iscc /DVersion=1.0.0-alpha.6.3 /DBinary=path\to\linewright.exe /DOutput=out installer\windows\linewright.iss

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Binary
  #define Binary "..\..\rust\target\release\linewright.exe"
#endif
#ifndef Output
  #define Output "."
#endif

[Setup]
AppId={{8C7D1E2A-5B3F-4E6A-9D21-6F0B3C4A7E15}
AppName=Linewright
AppVersion={#Version}
AppVerName=Linewright {#Version}
AppPublisher=Linewright
AppPublisherURL=https://github.com/idea90/linewright
AppSupportURL=https://github.com/idea90/linewright/issues
AppUpdatesURL=https://github.com/idea90/linewright/releases
DefaultDirName={autopf}\Linewright
DefaultGroupName=Linewright
DisableProgramGroupPage=yes
; Installs for the one user without an administrator, or for everyone when the installer is told to.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\..\LICENSE
SetupIconFile=..\..\rust\assets\linewright.ico
UninstallDisplayIcon={app}\linewright.exe
UninstallDisplayName=Linewright
OutputDir={#Output}
OutputBaseFilename=linewright-setup-windows-x86_64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; A Linewright still open is closed before its program is replaced, and opened again after.
CloseApplications=yes
RestartApplications=yes
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "associate"; Description: "Open &DXF and DWG drawings with Linewright"; GroupDescription: "Files:"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "linewright.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Linewright"; Filename: "{app}\linewright.exe"
Name: "{autodesktop}\Linewright"; Filename: "{app}\linewright.exe"; Tasks: desktopicon

[Registry]
; The kind of file, then each suffix pointing at it. HKA is the user's own registry or the machine's, as installed.
Root: HKA; Subkey: "Software\Classes\Linewright.Drawing"; ValueType: string; ValueData: "Drawing"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Linewright.Drawing\DefaultIcon"; ValueType: string; ValueData: "{app}\linewright.exe,0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Linewright.Drawing\shell\open\command"; ValueType: string; ValueData: """{app}\linewright.exe"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.dxf\OpenWithProgids"; ValueType: string; ValueName: "Linewright.Drawing"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.dwg\OpenWithProgids"; ValueType: string; ValueName: "Linewright.Drawing"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
; Linewright in the "Open with" list of every DXF and DWG file, whether or not it is made the one they open in.
Root: HKA; Subkey: "Software\Classes\Applications\linewright.exe\SupportedTypes"; ValueType: string; ValueName: ".dxf"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\linewright.exe\SupportedTypes"; ValueType: string; ValueName: ".dwg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\linewright.exe\shell\open\command"; ValueType: string; ValueData: """{app}\linewright.exe"" ""%1"""

[Run]
Filename: "{app}\linewright.exe"; Description: "{cm:LaunchProgram,Linewright}"; Flags: nowait postinstall skipifsilent
