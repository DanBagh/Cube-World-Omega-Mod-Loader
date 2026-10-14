; Item models: gives an item another model file.
; Assembled with ml64. The bytes are STUB in item_models.rs.
;
; Hook point, in the job that loads the model of an ordinary item:
;       lea  rdx, [rsp + N]             ; the model's path (pointer, length)
;       mov  rcx, rsi                   ; the model slot
;       xor  r8d, r8d                   ; replaced
;       call load_cub                   ; replaced
;       jmp  finished                   ; replaced
;
; rdi = the job plus 10h, holding a copy of the item (type, subtype, ...)
;
; An item whose type and subtype match an entry gets the entry's path, then the replaced
; instructions run as they were. The subtypes are made up by ground_model.asm.
;
; 0CCCCCCCCh is load_cub, 22222222h / 33333333h the item type / subtype from rdi,
; 0DDDDDDDDh where the client's jmp went.

.code

item_model PROC
        lea     r9, models
next_model:
        mov     eax, dword ptr [r9]                     ; -1 ends the table
        cmp     eax, -1
        je      load
        cmp     eax, dword ptr [rdi + 22222222h]
        jne     skip_model
        mov     eax, dword ptr [r9 + 4]
        cmp     eax, dword ptr [rdi + 33333333h]
        jne     skip_model
        movsxd  rax, dword ptr [r9 + 8]                 ; model file, relative to the entry
        add     rax, r9
        mov     qword ptr [rdx], rax                    ; the path load_cub is about to open
        jmp     load
skip_model:
        add     r9, 12
        jmp     next_model
load:
        xor     r8d, r8d
        db      0E8h                                    ; call <load_cub>
        dd      0CCCCCCCCh
        db      0E9h                                    ; jmp <finished>
        dd      0DDDDDDDDh
models:                                                 ; { type, subtype, path offset } ..., then -1
item_model ENDP

END
