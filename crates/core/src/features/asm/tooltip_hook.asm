; Tooltip lines: appends the lines of the matching table entries to an item's hint text.
; Assembled with ml64. The bytes are TOOLTIP_STUB in tooltips.rs.
;
; Hook point, in the client's tooltip hint builder, in front of its own control hints:
;       test bl, bl             ; replaced
;       je   <return>           ; replaced
;       mov  rcx, rdi           ; resumes here
;
; rsi = text { char text[128]; uint64 length; }, at most 127 characters
; rdi = item { uint32 type; uint32 subtype; ... }
; bl  = show the control hints
; rax, rcx, rdx and r8-r11 are free.
;
; 0CCCCCCCCh is the client's return path, 0DDDDDDDDh where it resumes.

.code

tooltip_hook PROC
        lea     r9, entries
        mov     eax, dword ptr [rdi]            ; item type
        mov     edx, dword ptr [rdi + 4]        ; item subtype
next_entry:
        mov     ecx, dword ptr [r9]             ; -1 ends the table
        cmp     ecx, -1
        je      done
        cmp     ecx, eax
        jne     skip
        mov     ecx, dword ptr [r9 + 4]         ; -1 matches any subtype
        cmp     ecx, -1
        je      matched
        cmp     ecx, edx
        jne     skip
matched:
        movsxd  rcx, dword ptr [r9 + 8]         ; line, relative to the entry
        add     rcx, r9
        mov     r8, qword ptr [rsi + 80h]       ; length so far
        test    r8, r8
        jz      copy
        cmp     r8, 7Eh
        jae     skip                            ; no room for a newline and a character
        mov     byte ptr [rsi + r8], 0Ah
        inc     r8
copy:
        mov     r10b, byte ptr [rcx]
        test    r10b, r10b
        jz      store
        cmp     r8, 7Eh
        ja      store
        mov     byte ptr [rsi + r8], r10b
        inc     rcx
        inc     r8
        jmp     copy
store:
        mov     byte ptr [rsi + r8], 0
        mov     qword ptr [rsi + 80h], r8
skip:
        add     r9, 12
        jmp     next_entry
done:
        test    bl, bl
        db      0Fh, 84h                        ; je <return>
        dd      0CCCCCCCCh
        db      0E9h                            ; jmp <resume>
        dd      0DDDDDDDDh
entries:                                        ; { type, subtype, line offset } ..., then -1
tooltip_hook ENDP

END
