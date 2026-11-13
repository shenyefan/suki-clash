; Compile with packaging/pack.py on the matching Windows runner.
#define CoreSHA256 GetSHA256OfFile(StageDir + "\resources\mihomo.exe")
#if FileExists(StageDir + "\resources\mihomo-alpha.exe")
#define AlphaCoreSHA256 GetSHA256OfFile(StageDir + "\resources\mihomo-alpha.exe")
#endif
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
ShowLanguageDialog=no

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Suki Clash"; Filename: "{app}\suki-clash.exe"
Name: "{autodesktop}\Suki Clash"; Filename: "{app}\suki-clash.exe"; Tasks: desktopicon

[Tasks]
Name: desktopicon; Description: "创建桌面快捷方式"; Flags: unchecked

[Registry]
Root: HKLM; Subkey: "Software\Classes\suki-clash"; ValueType: string; ValueData: "URL:Suki Clash"; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\suki-clash"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKLM; Subkey: "Software\Classes\suki-clash\shell\open\command"; ValueType: string; ValueData: """{app}\suki-clash.exe"" ""%1"""

[Run]
Filename: "{app}\suki-clash.exe"; Description: "启动 Suki Clash"; Flags: nowait postinstall runasoriginaluser; Check: ServiceReady

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
      MsgBox('Suki Clash 尚未退出。请关闭后重新运行安装程序。', mbError, MB_OK);
  finally
    CloseHandle(ProcessHandle);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  InstallArguments: String;
begin
  if CurStep = ssPostInstall then
  begin
    if not SaveStringToFile(ExpandConstant('{app}\inno-install'), 'inno', False) then
      RaiseException('无法记录安装模式。');
    { Stage the packaged cores explicitly; service discovery does not search resources. }
    InstallArguments := '--install --install-service --install-core "' +
      ExpandConstant('{app}\resources\mihomo.exe') + '" --sha256 {#CoreSHA256}';
#ifdef AlphaCoreSHA256
    InstallArguments := InstallArguments + ' --install-core "' +
      ExpandConstant('{app}\resources\mihomo-alpha.exe') + '" --sha256 {#AlphaCoreSHA256}';
#endif
    if not ExecAndLogOutput(ExpandConstant('{app}\suki-clash-service.exe'),
      InstallArguments, ExpandConstant('{app}'), SW_HIDE,
      ewWaitUntilTerminated, ResultCode, nil) then
      RaiseException('无法启动后台服务安装：' + SysErrorMessage(ResultCode));
    { Exit code 2 means the service is ready but a firewall rule failed. }
    if ResultCode = 2 then
      SuppressibleMsgBox('后台服务和内核已安装，但防火墙规则配置失败。使用虚拟网卡或允许局域网连接前，请修复服务。',
        mbError, MB_OK, IDOK)
    else if ResultCode <> 0 then
      RaiseException('后台服务安装失败（退出码 ' + IntToStr(ResultCode) +
        '）。请重新运行安装程序修复服务。');
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
    WizardForm.FinishedHeadingLabel.Caption := '后台服务安装未完成';
    WizardForm.FinishedLabel.Caption :=
      '应用文件已安装，但后台服务安装未完成。请重新运行安装程序修复服务。';
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
      SuppressibleMsgBox('无法启动后台服务卸载：' + SysErrorMessage(ResultCode),
        mbError, MB_OK, IDOK);
      Abort;
    end;
    if ResultCode <> 0 then
    begin
      SuppressibleMsgBox('后台服务卸载失败（退出码 ' + IntToStr(ResultCode) +
        '）。请修复服务后重试卸载。', mbError, MB_OK, IDOK);
      Abort;
    end;
  end;
end;
