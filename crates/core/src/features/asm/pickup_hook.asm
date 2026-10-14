; Pickup sounds: queues the sound of the matching table entry in place of the stock one.
; Assembled with ml64. The bytes are STUB in pickup_sounds.rs.
;
; Hook point, where the client's event loop handles "item picked up":
;       mov  word ptr [rcx + rax], <pickup id>  ; replaced
;       ...                                     ; resumes here
;
; rcx + rax = the new sound queue entry
; r8-r11 are free.
;
; The event record, holding the item (type, subtype, ...) and the count, is in a register
; that differs from build to build. The first instruction is assembled for rax and given
; the real one when the plan is made.
;
; The last eight pickups (type, subtype, count, sound id) are kept in the data area, after
; a count of all of them.
;
; 0CCCCCCCCh is the stock sound id, 22222222h / 33333333h the item type / subtype in the
; record, 44444444h the count, 0EEEEEEEEh the data area, 0DDDDDDDDh where the client resumes.

.code

pickup_sound_hook PROC
        db      4Ch, 8Bh, 0C8h                          ; mov r9, rax: rax stands for the record's register
        mov     r10d, dword ptr [r9 + 22222222h]        ; item type
        mov     r11d, dword ptr [r9 + 33333333h]        ; item subtype
        mov     r8d, dword ptr [r9 + 44444444h]
        push    r8                                      ; the count, for the record kept below
        mov     r8d, 0CCCCCCCCh
        lea     r9, entries
next_entry:
        cmp     dword ptr [r9], -1                      ; -1 ends the table
        je      done
        cmp     dword ptr [r9], r10d
        jne     skip
        cmp     dword ptr [r9 + 4], -1                  ; -1 matches any subtype
        je      matched
        cmp     dword ptr [r9 + 4], r11d
        jne     skip
matched:
        movzx   r8d, word ptr [r9 + 8]
        jmp     done
skip:
        add     r9, 12
        jmp     next_entry
done:
        mov     word ptr [rcx + rax], r8w               ; the replaced store

        db      4Ch, 8Dh, 0Dh                           ; lea r9, [rip + data area]
        dd      0EEEEEEEEh
        push    rax
        mov     eax, dword ptr [r9]
        inc     dword ptr [r9]
        and     eax, 7
        shl     eax, 4
        lea     r9, [r9 + rax + 16]                     ; one of eight 16-byte records
        pop     rax
        mov     dword ptr [r9], r10d
        mov     dword ptr [r9 + 4], r11d
        pop     r10                                     ; the count
        mov     dword ptr [r9 + 8], r10d
        mov     dword ptr [r9 + 12], r8d

        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
entries:                                                ; { type, subtype, sound id (word), 0 } ..., then -1
pickup_sound_hook ENDP

END
