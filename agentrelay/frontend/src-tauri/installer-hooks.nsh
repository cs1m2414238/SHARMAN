!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R9 SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}" "DisplayVersion"
  ${If} $R9 != ""
    nsis_tauri_utils::SemverCompare "${VERSION}" $R9
    Pop $R8
    ${If} $R8 = -1
      IfSilent agentrelay_downgrade_abort
      MessageBox MB_OK|MB_ICONSTOP "A newer Sharman version ($R9) is already installed. Downgrades are blocked to protect assistant and project memory."
      agentrelay_downgrade_abort:
      SetErrorLevel 2
      Quit
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  IfSilent agentrelay_postinstall_done
  MessageBox MB_YESNO|MB_ICONQUESTION "Create a Sharman shortcut on the desktop?" IDNO agentrelay_skip_desktop
    CreateShortcut "$DESKTOP\Sharman.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  agentrelay_skip_desktop:

  MessageBox MB_YESNO|MB_ICONQUESTION "Launch Sharman automatically when you sign in to Windows?" IDNO agentrelay_skip_startup
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Sharman" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --startup'
  agentrelay_skip_startup:
  agentrelay_postinstall_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Sharman"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AgentRelay"
  Delete "$DESKTOP\Sharman.lnk"
  Delete "$DESKTOP\AgentRelay.lnk"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  IfSilent agentrelay_keep_memory
  MessageBox MB_YESNO|MB_ICONQUESTION "Remove Sharman assistant settings, AgentRelay project memory, and backups? Choose No to preserve them for a future reinstall." IDNO agentrelay_keep_memory
    RMDir /r "$APPDATA\com.agentrelay.desktop"
  agentrelay_keep_memory:
!macroend
