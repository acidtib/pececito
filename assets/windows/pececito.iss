; Built by the release workflow: iscc /DVersion=<x.y.z> /DExePath=<pececito.exe> /O<dir> pececito.iss
#ifndef Version
  #error Pass the version with /DVersion=x.y.z
#endif
#ifndef ExePath
  #error Pass the built pececito.exe with /DExePath=<path>
#endif

[Setup]
; The AppId ties upgrades and the uninstaller to this app; never change it.
AppId={{D2052306-4124-4586-9FAE-FB21B8E1B9F6}
AppName=Pececito
AppVersion={#Version}
AppPublisher=Pececito
AppPublisherURL=https://github.com/acidtib/pececito
AppSupportURL=https://github.com/acidtib/pececito/issues
AppUpdatesURL=https://github.com/acidtib/pececito/releases
DefaultDirName={autopf}\Pececito
DefaultGroupName=Pececito
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputBaseFilename=Pececito-x86_64
SetupIconFile=..\icon\pececito.ico
UninstallDisplayIcon={app}\pececito.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=force
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "pececito.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\Pececito"; Filename: "{app}\pececito.exe"
Name: "{autodesktop}\Pececito"; Filename: "{app}\pececito.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\pececito.exe"; Description: "{cm:LaunchProgram,Pececito}"; Flags: nowait postinstall skipifsilent
