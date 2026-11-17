#define AppVersion GetEnv("GITHUB_REF_NAME")
[Setup]
AppId={{95E43578-6A97-44B1-A4E1-BE69B504F8EA}
AppName=Console Note
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Console Note
DefaultGroupName=Console Note
OutputDir=..
OutputBaseFilename=console_note-windows-x64-setup
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma
SolidCompression=yes
UninstallDisplayIcon={app}\console_note.exe
[Files]
Source: "target\x86_64-pc-windows-msvc\release\console_note.exe"; DestDir: "{app}"; Flags: ignoreversion
[Icons]
Name: "{group}\Console Note"; Filename: "{cmd}"; Parameters: "/K ""{app}\console_note.exe"""; WorkingDir: "{app}"
Name: "{group}\Uninstall Console Note"; Filename: "{uninstallexe}"
