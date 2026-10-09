!macro NSIS_HOOK_POSTINSTALL
  ; Tauri preserves existing shortcuts during /UPDATE, including their metadata.
  ${If} $NoShortcutMode <> 1
    ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      SetOutPath "$INSTDIR"
      CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\icons\icon.ico" 0
      !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      System::Call 'shell32::SHChangeNotify(i 0x00000002, i 0x00001005, w "$SMPROGRAMS\${PRODUCTNAME}.lnk", p 0)'
    ${EndIf}
  ${EndIf}
!macroend
