; Added sounds, loading: once the client has loaded its own sounds, loads each sound a mod
; adds into a slot of the loader's own, with the client's own functions.
; Assembled with ml64. The bytes are LOAD in added_sounds.rs.
;
; Hook point, the client's last call that loads one of its sounds:
;       call  load(manager, id, path)           ; replaced
;       ...                                     ; resumes here
;
; 0CCCCCCCCh is load, 0C1C1C1C1h init_from_file and 0C2C2C2C2h init_copy, all in the client.
; 11111111h is the number of added sounds, 12121212h the size of a slot, 13131313h the size
; of a sound, 14141414h where the manager keeps its audio engine and 15151515h the flags the
; client loads its sounds with. 0EEEEEEEEh is the slots in the data area, 0DDDDDDDDh where
; the client resumes. The paths follow the code, 64 bytes each.
;
; A slot: the number of voices ready (dword), the next voice to use (dword), 8 bytes, the
; sound, then its voices.

VOICES  equ 4
PATH    equ 64

.code

sound_load PROC
        push    rbx
        push    rsi
        push    rdi
        push    r12
        push    r13
        sub     rsp, 38h
        mov     rbx, rcx                                ; the audio manager
        db      0E8h                                    ; call load, as the client would
        dd      0CCCCCCCCh
        lea     r12, paths
        xor     esi, esi
next:
        cmp     esi, 11111111h
        jae     done
        imul    rdi, rsi, 12121212h
        db      48h, 8Dh, 05h                           ; lea rax, [rip + slots]
        dd      0EEEEEEEEh
        add     rdi, rax                                ; the slot
        lea     rcx, [rbx + 14141414h]                  ; the engine
        imul    rdx, rsi, PATH
        add     rdx, r12                                ; the path
        mov     r8d, 15151515h
        xor     r9d, r9d
        mov     qword ptr [rsp + 20h], 0
        lea     rax, [rdi + 10h]
        mov     qword ptr [rsp + 28h], rax              ; the sound
        db      0E8h                                    ; call init_from_file
        dd      0C1C1C1C1h
        test    eax, eax
        jnz     skip
        xor     r13d, r13d
voice:
        lea     rcx, [rbx + 14141414h]
        lea     rdx, [rdi + 10h]
        xor     r8d, r8d
        xor     r9d, r9d
        lea     eax, [r13 + 1]
        imul    rax, rax, 13131313h
        lea     rax, [rdi + rax + 10h]
        mov     qword ptr [rsp + 20h], rax              ; a voice
        db      0E8h                                    ; call init_copy
        dd      0C2C2C2C2h
        test    eax, eax
        jnz     voiced
        inc     r13d
        cmp     r13d, VOICES
        jb      voice
voiced:
        mov     dword ptr [rdi], r13d                   ; voices ready
skip:
        inc     esi
        jmp     next
done:
        add     rsp, 38h
        pop     r13
        pop     r12
        pop     rdi
        pop     rsi
        pop     rbx
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
paths:
sound_load ENDP

END
