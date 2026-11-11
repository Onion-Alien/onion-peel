; OnionPeelSetup.exe: the download. A normal per-user Inno Setup installer, so the app's own
; exe never installs itself (a self-installing exe looks like a dropper to virus scanners'
; machine-learning guesses).
;
;   - no admin needed: %LOCALAPPDATA%\Programs\Onion Peel, the same folder as 1.0's
;     self-install
;   - one page: what it is, a picture of the snip screen, and a "Start with Windows" box
;     (ticked; an update keeps the last install's choice). Then installing, then "All
;     done". Onion Peel starts at the end of every install (it has no window, so there's
;     nothing to "open")
;   - Inno's plain "modern" look with no side panel or header logo: the snip screen
;     picture is the only image. Setup's own icon is Inno's, and it starts with Inno's
;     64-bit loader: see "Virus scanners" at [Setup]
;   - Start menu: "Onion Peel" and "Uninstall Onion Peel"; Settings > Apps entry
;   - a running copy is asked to close first (onion-peel.exe --quit), on install and
;     uninstall, so its exe can be replaced or removed
;   - updating over 1.0's self-install: its own Settings > Apps entry is removed, so
;     there's one entry, and its startup choice is kept
;
; Built by scripts\build-release.ps1 (needs Inno Setup 6.7 or newer:
; winget install JRSoftware.InnoSetup), which builds the exe and draws the pictures first
; (scripts\make_installer_art.py: the snip screen picture at each display scale).
; /DPREVIEW is a look at the pages only: it installs nothing
; (scripts\installer_pictures.ps1).

#define AppName "Onion Peel"
#define AppExe "onion-peel.exe"
; the Run and old Uninstall value / key name
#define RegName "OnionPeel"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{62F5E021-5387-42F1-BB03-740C0ECFC44B}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName}
; files keep their UTC time stamps: local ones would give away the build PC's time zone
TimeStampsInUTC=yes
AppPublisher=OnionAlien
AppPublisherURL=https://github.com/Onion-Alien/onion-peel
AppSupportURL=https://github.com/Onion-Alien/onion-peel/issues
AppUpdatesURL=https://github.com/Onion-Alien/onion-peel/releases
AppCopyright=OnionAlien, GPL-3.0
AppComments=Win+Shift+S region snip to the clipboard
; a named, versioned setup file rather than a blank one (Properties > Details), which
; also helps machine-learning virus scanners that distrust those
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}
VersionInfoCompany=OnionAlien
VersionInfoDescription={#AppName} Setup
VersionInfoCopyright=OnionAlien, GPL-3.0
#ifdef PREVIEW
DefaultDirName={tmp}\preview
Uninstallable=no
#else
DefaultDirName={localappdata}\Programs\{#AppName}
#endif
DefaultGroupName={#AppName}
PrivilegesRequired=lowest
DisableWelcomePage=yes
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
; Virus scanners: a setup this small is called malware by a few machine-learning
; scanners on VirusTotal whatever is inside it (the app's own exe scans clean). Tested one
; change at a time, three things each cost a flag, so don't bring one back without
; scanning the result (scripts\vt_scan.py):
;   - Inno's 32-bit loader (Skyhigh, Cylance, DeepInstinct): UseSetupLdr=x64
;   - an icon of our own on the setup file (APEX), so no SetupIconFile: Setup keeps
;     Inno's icon. The installed app has its own icon as before
;   - the Windows 11 look, "dynamic windows11" (CrowdStrike, most builds): plain modern
UseSetupLdr=x64
WizardStyle=modern
WizardSizePercent=100
; no side panel or header logo (InitializeWizard adds the snip screen picture instead)
WizardImageFile=
WizardSmallImageFile=
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir=..\target\installer
OutputBaseFilename=OnionPeelSetup
; zip, not solid lzma: Microsoft's machine-learning scanner calls many solid-lzma
; installers Trojan:Win32/Wacatac.B!ml whatever is inside
Compression=zip
SolidCompression=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Messages]
SetupWindowTitle=Install %1
WizardSelectTasks=Install Onion Peel
SelectTasksDesc=Peel a piece off your screen
SelectTasksLabel2=Press Win+Shift+S, pick an area, and it's on the clipboard.%n%nNo window, no tray icon, no files saved, and it never goes online.
FinishedHeadingLabel=All done!
FinishedLabel=Onion Peel is installed and running. Press Win+Shift+S to snip.%n%nTo start or uninstall it later, search the Start menu for Onion Peel.
FinishedLabelNoIcons=Onion Peel is installed and running. Press Win+Shift+S to snip.%n%nTo start or uninstall it later, search the Start menu for Onion Peel.
ClickFinish=

[Tasks]
Name: "startup"; Description: "Start with Windows"

[Files]
; the snip screen picture, for the pages only (never installed)
Source: "..\target\installer\snip-light-*.png"; Flags: dontcopy
#ifndef PREVIEW

[Dirs]
; 1.0's self-install made the folder, so Setup wouldn't count it as its own and the
; uninstaller would leave it behind, empty. Removed when empty, never with anything else
; still in it.
Name: "{app}"; Flags: uninsalwaysuninstall

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Snip part of the screen to the clipboard with Win+Shift+S"
Name: "{autoprograms}\Uninstall {#AppName}"; Filename: "{uninstallexe}"; Comment: "Remove Onion Peel from this PC"

[Registry]
; "Start Onion Peel when Windows starts": set when ticked, removed when unticked (on an
; update too) and on uninstall
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#RegName}"; ValueData: """{app}\{#AppExe}"""; Tasks: startup; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "{#RegName}"; Tasks: not startup; Flags: deletevalue dontcreatekey uninsdeletevalue
; 1.0's own Settings > Apps entry (its exe installed itself): this installer's replaces it
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\{#RegName}"; ValueType: none; Flags: deletekey dontcreatekey

[Run]
; started on every install, silent updates too: it was asked to close before the files
; were replaced
Filename: "{app}\{#AppExe}"; Flags: nowait

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--quit"; RunOnceId: "QuitApp"; Flags: runhidden waituntilterminated
#endif

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  OldEntry = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#RegName}';

var
  StartupChecked: Boolean;
  TasksPic, DonePic: TBitmapImage;

// The snip screen picture for this screen's scale, made to size by make_installer_art.py
// so it's never stretched
function SnipPicture: String;
var
  Pct: Integer;
begin
  Pct := ScaleX(100);
  if Pct < 113 then Pct := 100
  else if Pct < 138 then Pct := 125
  else if Pct < 163 then Pct := 150
  else if Pct < 188 then Pct := 175
  else Pct := 200;
  Result := 'snip-light-' + IntToStr(Pct) + '.png';
  ExtractTemporaryFile(Result);
  Result := ExpandConstant('{tmp}\') + Result;
end;

function AddPicture(Parent: TWinControl): TBitmapImage;
begin
  Result := TBitmapImage.Create(WizardForm);
  Result.Parent := Parent;
  Result.AutoSize := True;
  Result.PngImage.LoadFromFile(SnipPicture);
end;

procedure InitializeWizard;
var
  Spare: Integer;
begin
  TasksPic := AddPicture(WizardForm.SelectTasksPage);
  DonePic := AddPicture(WizardForm.FinishedPage);
  // Inno puts the pages' content well below the heading: bring it up to a normal gap (the
  // heading's panel too, or it would cover the top of the page)
  WizardForm.MainPanel.Height := WizardForm.MainPanel.Height - ScaleY(12);
  WizardForm.InnerNotebook.Top := WizardForm.InnerNotebook.Top - ScaleY(12);
  WizardForm.InnerNotebook.Height := WizardForm.InnerNotebook.Height + ScaleY(12);
  // and its window is taller than these two short pages need: shorten it to fit the
  // picture, so there's no empty band above the buttons
  Spare := WizardForm.SelectTasksPage.ClientHeight - TasksPic.Height - ScaleY(8);
  if Spare > 0 then
    WizardForm.ClientHeight := WizardForm.ClientHeight - Spare;
end;

// Text on the left, the picture on the right: side by side as one block under the heading,
// the text centred against the picture
procedure LayOut(CurPageID: Integer);
var
  Gap, Left, Right, TextH, BlockH, Top: Integer;
begin
  Gap := ScaleX(24);
  if CurPageID = wpSelectTasks then
  begin
    TasksPic.Left := WizardForm.SelectTasksPage.ClientWidth - TasksPic.Width;
    Right := TasksPic.Left - Gap;
    WizardForm.SelectTasksLabel.Width := Right - WizardForm.SelectTasksLabel.Left;
    WizardForm.SelectTasksLabel.AdjustHeight;
    WizardForm.TasksList.Width := Right - WizardForm.TasksList.Left;
    WizardForm.TasksList.Height := ScaleY(24);   // its one box
    TextH := WizardForm.SelectTasksLabel.Height + ScaleY(16) + WizardForm.TasksList.Height;
    BlockH := TasksPic.Height;
    if TextH > BlockH then BlockH := TextH;
    Top := 0;   // right under the heading (InitializeWizard moved the page up and shortened the window)
    TasksPic.Top := Top + (BlockH - TasksPic.Height) div 2;
    WizardForm.SelectTasksLabel.Top := Top + (BlockH - TextH) div 2;
    WizardForm.TasksList.Top := WizardForm.SelectTasksLabel.Top + WizardForm.SelectTasksLabel.Height + ScaleY(16);
  end
  else if CurPageID = wpFinished then
  begin
    DonePic.Left := WizardForm.FinishedPage.ClientWidth - DonePic.Width - ScaleX(40);
    DonePic.Top := (WizardForm.FinishedPage.ClientHeight - DonePic.Height) div 2;
    Right := DonePic.Left - Gap;
    // Inno leaves room on the left for the side panel this setup doesn't have
    Left := ScaleX(40);
    WizardForm.FinishedHeadingLabel.Left := Left;
    WizardForm.FinishedHeadingLabel.Width := Right - Left;
    WizardForm.FinishedHeadingLabel.Height := ScaleY(30);   // one line (Inno's box is much taller)
    WizardForm.FinishedLabel.Left := Left;
    WizardForm.FinishedLabel.Width := Right - Left;
    WizardForm.FinishedLabel.AdjustHeight;
    TextH := WizardForm.FinishedHeadingLabel.Height + ScaleY(12) + WizardForm.FinishedLabel.Height;
    WizardForm.FinishedHeadingLabel.Top := DonePic.Top + (DonePic.Height - TextH) div 2;
    WizardForm.FinishedLabel.Top := WizardForm.FinishedHeadingLabel.Top + WizardForm.FinishedHeadingLabel.Height + ScaleY(12);
    // Finish is the only button left: put it at the right, where Cancel was
    WizardForm.NextButton.Left := WizardForm.CancelButton.Left;
  end;
end;

// The page's button says Install: there's no Ready page after it
procedure CurPageChanged(CurPageID: Integer);
begin
  LayOut(CurPageID);
  if CurPageID <> wpSelectTasks then
    exit;
  WizardForm.NextButton.Caption := SetupMessage(msgButtonInstall);
  // An update over 1.0 (installed by its own exe, so this installer has no record of the
  // box): keep what was chosen then
  if not StartupChecked and RegKeyExists(HKCU, OldEntry) and
     not RegValueExists(HKCU, RunKey, '{#RegName}') then
    WizardSelectTasks('!startup');
  StartupChecked := True;
end;

// Ask a running copy to close, so its exe can be replaced
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
  Exe: String;
begin
  Result := '';
  Exe := ExpandConstant('{app}\{#AppExe}');
  if FileExists(Exe) then
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;
