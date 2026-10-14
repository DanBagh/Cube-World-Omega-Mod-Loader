; Added sounds, playing: a queue entry with the id of an added sound is played from the
; loader's own voices, with the client's own functions.
; Assembled with ml64. The bytes are PLAY in added_sounds.rs.
;
; Hook point, the start of the client's play(manager, entry), after its prologue:
;       movzx eax, word ptr [rdx]               ; replaced
;       cmp   rax, <table size>                 ; replaced
;       je    <way out>                         ; taken when the flags say equal
;
; rcx = the audio manager, rdx = the queue entry. The prologue has saved every callee-saved
; register and xmm6 to xmm9, and the way out restores them, so they are free here. rsp is
; 16-byte aligned, with shadow space at [rsp].
;
; 0C1C1C1C1h is is_playing, 0C2C2C2C2h stop, 0C3C3C3C3h seek, 0C4C4C4C4h attach, 0C5C5C5C5h
; set_spatialization, 0C6C6C6C6h set_pitch, 0C7C7C7C7h set_volume and 0C8C8C8C8h start, all
; in the client. 11111111h is the number of added sounds, 12121212h the size of a slot,
; 13131313h the size of a sound, 14141414h what the manager attaches voices to, 15151515h the
; entry's pitch, 16161616h the entry's volume and 17171717h the manager's volume for the
; sounds' category. 0EEEEEEEEh is the slots in the data area, 0DDDDDDDDh where the client
; resumes. The replaced instructions are copied in over the nine 90h bytes.

FIRST   equ 8000h

.code

sound_play PROC
        cmp     word ptr [rdx], FIRST
        jae     added
        db      9 dup (90h)                             ; the replaced instructions
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
added:
        mov     rsi, rcx
        mov     rdi, rdx
        movzx   ebx, word ptr [rdi]
        sub     ebx, FIRST
        cmp     ebx, 11111111h
        jae     finish
        imul    rbx, rbx, 12121212h
        db      4Ch, 8Dh, 2Dh                           ; lea r13, [rip + slots]
        dd      0EEEEEEEEh
        add     rbx, r13                                ; the slot
        mov     r12d, dword ptr [rbx]                   ; voices ready
        test    r12d, r12d
        jz      finish
        ; from the next voice on, the first that is not playing, else the next one
        mov     r15d, dword ptr [rbx + 4]
        cmp     r15d, r12d
        jb      search
        xor     r15d, r15d
search:
        mov     r13d, r12d                              ; voices left to try
try:
        lea     eax, [r15 + 1]
        imul    rcx, rax, 13131313h
        lea     rcx, [rbx + rcx + 10h]
        db      0E8h                                    ; call is_playing
        dd      0C1C1C1C1h
        test    eax, eax
        jz      chosen
        inc     r15d
        cmp     r15d, r12d
        jb      tried
        xor     r15d, r15d
tried:
        dec     r13d
        jnz     try
chosen:
        lea     eax, [r15 + 1]
        imul    r14, rax, 13131313h
        lea     r14, [rbx + r14 + 10h]                  ; the voice
        mov     rcx, r14
        db      0E8h                                    ; call is_playing
        dd      0C1C1C1C1h
        test    eax, eax
        jz      rewind
        mov     rcx, r14
        db      0E8h                                    ; call stop
        dd      0C2C2C2C2h
rewind:
        mov     rcx, r14
        xor     edx, edx
        db      0E8h                                    ; call seek
        dd      0C3C3C3C3h
        lea     rcx, [rsi + 14141414h]
        mov     rdx, r14
        xor     r8d, r8d                                ; not placed in the world
        db      0E8h                                    ; call attach
        dd      0C4C4C4C4h
        test    eax, eax
        jnz     finish
        mov     rcx, r14
        xor     edx, edx
        db      0E8h                                    ; call set_spatialization
        dd      0C5C5C5C5h
        movss   xmm1, dword ptr [rdi + 15151515h]       ; the entry's pitch
        mov     rcx, r14
        db      0E8h                                    ; call set_pitch
        dd      0C6C6C6C6h
        movss   xmm1, dword ptr [rdi + 16161616h]       ; the entry's volume
        mulss   xmm1, dword ptr [rsi + 17171717h]       ; times its category's
        mov     rcx, r14
        db      0E8h                                    ; call set_volume
        dd      0C7C7C7C7h
        mov     rcx, r14
        db      0E8h                                    ; call start
        dd      0C8C8C8C8h
        inc     r15d
        cmp     r15d, r12d
        jb      keep
        xor     r15d, r15d
keep:
        mov     dword ptr [rbx + 4], r15d
finish:
        cmp     eax, eax                                ; equal: the client takes its way out
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
sound_play ENDP

END
