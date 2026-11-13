; Compile with packaging/pack.py on the matching Windows runner.
[Setup]
AppId={{CF241840-F76C-4AF9-88DC-2DFA874DF04B}
AppName=Suki Clash
AppVersion={#AppVersion}
AppPublisher=Moenya
DefaultDirName={autopf}\Suki Clash
DefaultGroupName=Suki Clash
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed={#AppArch}
ArchitecturesInstallIn64BitMode={#AppArch}
OutputDir={#OutputDir}
OutputBaseFilename=SukiClash-{#AppVersion}-{#Channel}-Setup
SetupIconFile=..\..\assets\logo.ico
UninstallDisplayIcon={app}\suki-clash.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
UninstallLogging=yes

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Suki Clash"; Filename: "{app}\suki-clash.exe"
Name: "{autodesktop}\Suki Clash"; Filename: "{app}\suki-clash.exe"; Tasks: desktopicon

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Registry]
Root: HKLM; Subkey: "Software\Classes\suki-clash"; ValueType: string; ValueData: "URL:Suki Clash"; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\suki-clash"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKLM; Subkey: "Software\Classes\suki-clash\shell\open\command"; ValueType: string; ValueData: """{app}\suki-clash.exe"" ""%1"""

[Run]
Filename: "{app}\suki-clash.exe"; Description: "Launch Suki Clash"; Flags: nowait postinstall runasoriginaluser; Check: ServiceReady

[UninstallDelete]
Type: files; Name: "{app}\inno-install"

[Code]
var
  ServiceInstalled: Boolean;

function ServiceReady(): Boolean;
begin
  Result := ServiceInstalled;
end;

function OpenProcess(Access: LongWord; InheritHandle: Boolean; ProcessId: LongWord): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForSingleObject(Handle: THandle; Milliseconds: LongWord): LongWord;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

function InitializeSetup(): Boolean;
var
  ProcessId: Integer;
  ProcessHandle: THandle;
begin
  Result := True;
  ProcessId := StrToIntDef(ExpandConstant('{param:SUKIWAITPID|0}'), 0);
  if ProcessId = 0 then Exit;
  ProcessHandle := OpenProcess($00100000, False, ProcessId);
  if ProcessHandle = 0 then Exit;
  try
    Result := WaitForSingleObject(ProcessHandle, 60000) = 0;
    if not Result then
      MsgBox('Suki Clash did not exit. Close it and run the installer again.', mbError, MB_OK);
  finally
    CloseHandle(ProcessHandle);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    if not SaveStringToFile(ExpandConstant('{app}\inno-install'), 'inno', False) then
      RaiseException('Unable to record the installation mode.');
    if not ExecAndLogOutput(ExpandConstant('{app}\suki-clash-service.exe'),
      '--install --install-service', ExpandConstant('{app}'), SW_HIDE,
      ewWaitUntilTerminated, ResultCode, nil) then
      RaiseException('Unable to start service installation: ' + SysErrorMessage(ResultCode));
    { Exit code 2 means the service is ready but a firewall rule failed. }
    if ResultCode = 2 then
      SuppressibleMsgBox('The service is ready, but its firewall rule could not be configured. Repair the service before using TUN or LAN access.',
        mbError, MB_OK, IDOK)
    else if ResultCode <> 0 then
      RaiseException('Service installation failed (exit code ' + IntToStr(ResultCode) +
        '). Run the installer again to repair the service.');
    ServiceInstalled := True;
  end;
end;

function GetCustomSetupExitCode(): Integer;
begin
  if ServiceInstalled then Result := 0 else Result := 1;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpFinished) and not ServiceInstalled then
  begin
    WizardForm.FinishedHeadingLabel.Caption := 'Service installation needs repair';
    WizardForm.FinishedLabel.Caption :=
      'The application files were installed, but service installation did not finish. Run the installer again to repair the service.';
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    if not ExecAndLogOutput(ExpandConstant('{app}\suki-clash-service.exe'),
      '--uninstall', ExpandConstant('{app}'), SW_HIDE,
      ewWaitUntilTerminated, ResultCode, nil) then
    begin
      SuppressibleMsgBox('Unable to start service removal: ' + SysErrorMessage(ResultCode),
        mbError, MB_OK, IDOK);
      Abort;
    end;
    if ResultCode <> 0 then
    begin
      SuppressibleMsgBox('Service removal failed (exit code ' + IntToStr(ResultCode) +
        '). Repair the service and retry uninstalling.', mbError, MB_OK, IDOK);
      Abort;
    end;
  end;
end;
