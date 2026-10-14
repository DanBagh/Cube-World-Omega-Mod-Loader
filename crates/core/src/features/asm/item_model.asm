; Item models: gives an item another model file, and marks voxels of a model as glowing.
; Assembled with ml64. The bytes are STUB in item_models.rs.
;
; Hook point, in the job that loads the model of an ordinary item:
;       lea  rdx, [rsp + N]             ; the model's path (pointer, length)
;       mov  rcx, rsi                   ; the model slot
;       xor  r8d, r8d                   ; replaced: 0 builds the mesh right away
;       call load_cub                   ; replaced
;       jmp  finished                   ; replaced
;
; rsi = model slot: its size, the colours, one "glows" bit per voxel
; rdi = the job plus 10h, holding a copy of the item (type, subtype, ...)
;
; The client loads with r8d = 1 for the items whose voxels it changes itself, then runs
; `mov rcx, rsi / mov dl, 1 / call build_mesh` in front of `finished`. This takes that
; route for every item: load only, set the bits of a matching entry, go to that mesh call.
;
; models: an item whose type and subtype match gets the entry's path. The subtypes are
; made up by ground_model.asm.
; glow: an entry applies when the item matches and the loaded model has its size.
;
; The data area counts the models loaded (+0A0h) and the ones given glowing voxels (+0A4h).
;
; 0CCCCCCCCh is load_cub, 22222222h / 33333333h the item type / subtype from rdi,
; 66666666h the size and 55555555h the voxel bits in the slot, 0EEEEEEEEh the data area,
; 0DDDDDDDDh the client's mesh call. The two `lea r9` get the two tables.

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
        mov     r8d, 1                                  ; load, do not mesh yet
        db      0E8h                                    ; call <load_cub>
        dd      0CCCCCCCCh

        db      4Ch, 8Dh, 15h                           ; lea r10, [rip + data area]
        dd      0EEEEEEEEh
        lock inc dword ptr [r10 + 0A0h]

        lea     r9, glow
next_entry:
        mov     eax, dword ptr [r9]                     ; -1 ends the table
        cmp     eax, -1
        je      done
        cmp     eax, dword ptr [rdi + 22222222h]
        jne     skip
        mov     eax, dword ptr [r9 + 4]                 ; -1 matches any subtype
        cmp     eax, -1
        je      check_size
        cmp     eax, dword ptr [rdi + 33333333h]
        jne     skip
check_size:
        lea     r11, [rsi + 66666666h]                  ; the slot's size (x, y, z)
        mov     eax, dword ptr [r9 + 8]
        cmp     eax, dword ptr [r11]
        jne     skip
        mov     eax, dword ptr [r9 + 12]
        cmp     eax, dword ptr [r11 + 4]
        jne     skip
        mov     eax, dword ptr [r9 + 16]
        cmp     eax, dword ptr [r11 + 8]
        jne     skip

        mov     ecx, dword ptr [r9 + 20]                ; mask length in quadwords
        movsxd  rdx, dword ptr [r9 + 24]                ; mask, relative to the entry
        add     rdx, r9
        lea     r8, [rsi + 55555555h]                   ; the slot's voxel bits
set_bits:
        test    ecx, ecx
        jz      marked
        mov     rax, qword ptr [rdx]
        or      qword ptr [r8], rax
        add     rdx, 8
        add     r8, 8
        dec     ecx
        jmp     set_bits
marked:
        lock inc dword ptr [r10 + 0A4h]
        jmp     done
skip:
        add     r9, 28
        jmp     next_entry
done:
        db      0E9h                                    ; jmp <mesh call>
        dd      0DDDDDDDDh
models:                                                 ; { type, subtype, path offset } ..., then -1
glow:                                                   ; { type, subtype, x, y, z, quadwords, mask offset } ..., then -1
item_model ENDP

END
