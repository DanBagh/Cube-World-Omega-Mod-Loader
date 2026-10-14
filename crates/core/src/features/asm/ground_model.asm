; Models by amount: asks for another model when enough of an item lie on the ground.
; Assembled with ml64. The bytes are STUB in ground_models.rs.
;
; Hook point, where the client fetches the model of an item on the ground:
;       call model_of_item              ; replaced
;
; r8 = the item (20 bytes) inside its ground record, with the amount a fixed distance on
;
; model_of_item keeps one model per distinct item and copies what it is given. So it gets
; a copy of the item, and for an entry's type with at least its amount, the entry's
; made-up subtype in that copy. item_model.asm turns the subtype into a model file. The
; record itself is not changed.
;
; Entries have the highest amount first. The first that fits is used.
;
; SSE only: the baseline client runs on processors without AVX.
;
; 44444444h is where the amount is from the item, 0CCCCCCCCh model_of_item, 0DDDDDDDDh
; where the client resumes.

.code

ground_model PROC
        sub     rsp, 40h                                ; home space, and the copy above it
        movups  xmm0, xmmword ptr [r8]
        mov     eax, dword ptr [r8 + 10h]
        movups  xmmword ptr [rsp + 20h], xmm0
        mov     dword ptr [rsp + 30h], eax
        lea     r10, entries
next_entry:
        mov     eax, dword ptr [r10]                    ; -1 ends the table
        cmp     eax, -1
        je      look
        cmp     eax, dword ptr [rsp + 20h]
        jne     skip
        movzx   eax, word ptr [r8 + 44444444h]
        cmp     eax, dword ptr [r10 + 4]                ; at least this many
        jb      skip
        mov     eax, dword ptr [r10 + 8]
        mov     dword ptr [rsp + 24h], eax
        jmp     look
skip:
        add     r10, 12
        jmp     next_entry
look:
        lea     r8, [rsp + 20h]
        db      0E8h                                    ; call <model_of_item>
        dd      0CCCCCCCCh
        add     rsp, 40h
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
entries:                                                ; { type, least amount, subtype } ..., then -1
ground_model ENDP

END
