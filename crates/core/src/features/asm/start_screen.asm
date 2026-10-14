; Start screen: puts a note after the game's version in the corner of the start screen.
; Assembled with ml64. The bytes are STUB in start_screen.rs.
;
; Hook point, where the start screen hands the version text to the text drawing:
;       mov  qword ptr [rbp + N], rcx           ; replaced: the text
;       mov  qword ptr [rbp + N + 8], rax       ; replaced: its length
;       ...                                     ; resumes here
;
; The text is copied into the data area with the note after it, and the replaced stores
; are made with the copy. The client sets rdx, r8 and r9 again before it draws, and
; r10 and r11 are free.
;
; 0EEEEEEEEh is the data area, 0DDDDDDDDh where the client resumes. The replaced stores
; are copied in over the eight 90h bytes. The note follows the code: its length as a
; dword, then its text.

MAX_TEXT equ 64

.code

start_screen PROC
        db      4Ch, 8Dh, 15h                           ; lea r10, [rip + data area]
        dd      0EEEEEEEEh
        xor     r11d, r11d
copy:
        cmp     r11, rax
        jae     copied
        cmp     r11, MAX_TEXT
        jae     copied
        mov     dl, byte ptr [rcx + r11]
        mov     byte ptr [r10 + r11], dl
        inc     r11
        jmp     copy
copied:
        lea     r8, note
        mov     r9d, dword ptr [r8]
        add     r8, 4
append:
        test    r9d, r9d
        jz      done
        mov     dl, byte ptr [r8]
        mov     byte ptr [r10 + r11], dl
        inc     r8
        inc     r11
        dec     r9d
        jmp     append
done:
        mov     rcx, r10
        mov     rax, r11
        db      8 dup (90h)                             ; the replaced stores
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
note:
start_screen ENDP

END
