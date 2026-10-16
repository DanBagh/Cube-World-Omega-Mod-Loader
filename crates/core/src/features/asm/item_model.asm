; Item models: gives an item another model file.
; Assembled with ml64. The bytes are STUB in item_models.rs.
;
; Hook point, the start of the function the model of every item is fetched through:
;       push rbp                        ; replaced
;       push r15                        ; replaced
;       push r14                        ; replaced
;       ...                             ; resumes here
;
; r8 = the item (type, subtype, ...), r9 = the model file it is to get: the characters,
; then their count. The client keeps one model per file, so an item of an entry gets the
; entry's file name before the client looks it up. The subtypes are made up by
; ground_model.asm. Nothing else is touched, and every argument register is kept.
;
; 55555555h is where the file name's length is kept, 0DDDDDDDDh where the client resumes.

.code

item_model PROC
        lea     r10, models
next_model:
        mov     eax, dword ptr [r10]                    ; -1 ends the table
        cmp     eax, -1
        je      done
        cmp     eax, dword ptr [r8]
        jne     skip_model
        mov     eax, dword ptr [r10 + 4]
        cmp     eax, dword ptr [r8 + 4]
        jne     skip_model
        movsxd  rax, dword ptr [r10 + 8]                ; model file, relative to the entry
        add     rax, r10
        xor     r11d, r11d
copy:
        movzx   r10d, byte ptr [rax + r11]              ; its end is copied too
        mov     byte ptr [r9 + r11], r10b
        test    r10d, r10d
        jz      copied
        inc     r11
        jmp     copy
copied:
        mov     qword ptr [r9 + 55555555h], r11         ; and its length
        jmp     done
skip_model:
        add     r10, 12
        jmp     next_model
done:
        push    rbp                                     ; the replaced instructions
        push    r15
        push    r14
        db      0E9h                                    ; jmp <resume>
        dd      0DDDDDDDDh
models:                                                 ; { type, subtype, path offset } ..., then -1
item_model ENDP

END
