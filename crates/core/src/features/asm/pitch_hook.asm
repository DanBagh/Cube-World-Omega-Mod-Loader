; Sound pitch: gives the sounds in the table a pitch from their own range as they are played.
; Assembled with ml64. The bytes are STUB in sound_pitch.rs.
;
; Hook point, the loop that plays the queued sounds once a frame:
;       mov  rcx, rsi                   ; replaced
;       mov  rdx, rbx                   ; replaced
;       call play_entry                 ; replaced
;       add  rbx, 38h / dec rdi / jnz   ; resumes here
;
; rbx = queue entry { sound id (word), ..., pitch (float at +20h), volume (float at +24h) }
; rax, rcx, rdx, r8-r11 and xmm0 are free.
;
; SSE only: the baseline client runs on processors without AVX.
;
; 0CCCCCCCCh is play_entry, 0DDDDDDDDh where the client resumes.

.code

pitch_hook PROC
        movzx   r8d, word ptr [rbx]
        lea     r9, entries
next_entry:
        movzx   ecx, word ptr [r9]
        cmp     ecx, 0FFFFh                             ; ends the table
        je      play
        cmp     ecx, r8d
        je      matched
        add     r9, 12
        jmp     next_entry
matched:
        rdtsc                                           ; low bits differ from one sound to the next
        and     eax, 3FFh
        cvtsi2ss xmm0, eax
        mulss   xmm0, dword ptr [r9 + 8]                ; (highest - lowest) / 1023
        addss   xmm0, dword ptr [r9 + 4]                ; lowest
        movss   dword ptr [rbx + 20h], xmm0
play:
        mov     rcx, rsi
        mov     rdx, rbx
        db      0E8h                                    ; call <play_entry>
        dd      0CCCCCCCCh
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
entries:                                                ; { id (word), 0 (word), lowest (float), step (float) } ..., then 0FFFFh
pitch_hook ENDP

END
