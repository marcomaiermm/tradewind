#!/usr/bin/env python3
"""Read Python 3.4 marshal bytecode without executing the inspected application."""
import argparse
import json
import struct
import zipfile


class Reader:
    def __init__(self, data):
        self.data, self.pos, self.refs = data, 0, []

    def take(self, n):
        result = self.data[self.pos:self.pos + n]
        self.pos += n
        return result

    def int(self):
        return struct.unpack('<i', self.take(4))[0]

    def read(self):
        tag = self.take(1)[0]
        flag, tag = tag & 128, chr(tag & 127)
        index = len(self.refs)
        if flag:
            self.refs.append(None)
        if tag in 'N0': value = None
        elif tag == 'F': value = False
        elif tag == 'T': value = True
        elif tag == 'i': value = self.int()
        elif tag == 'r': value = self.refs[self.int()]
        elif tag == 'g': value = struct.unpack('<d', self.take(8))[0]
        elif tag == 'l':
            n = self.int()
            value = sum(struct.unpack('<H', self.take(2))[0] << (15 * i) for i in range(abs(n)))
            if n < 0: value = -value
        elif tag in 'sutaAzZ':
            n = self.take(1)[0] if tag in 'zZ' else self.int()
            raw = self.take(n)
            value = raw if tag == 's' else raw.decode('utf-8')
        elif tag in '([':
            value = [self.read() for _ in range(self.int())]
        elif tag == ')': value = [self.read() for _ in range(self.take(1)[0])]
        elif tag == '{':
            value = {}
            while True:
                key = self.read()
                if key is None: break
                value[key] = self.read()
        elif tag == 'c':
            value = {'kind': 'code'}
            if flag: self.refs[index] = value
            for k in ['argcount', 'kwonlyargcount', 'nlocals', 'stacksize', 'flags']: value[k] = self.int()
            for k in ['code', 'consts', 'names', 'varnames', 'freevars', 'cellvars', 'filename', 'name']: value[k] = self.read()
            value['firstlineno'] = self.int()
            value['lnotab'] = self.read()
        else: raise ValueError(f'Unsupported marshal tag {tag!r} at {self.pos - 1}')
        if flag: self.refs[index] = value
        return value


OPS = {0:'STOP_CODE',1:'POP_TOP',2:'ROT_TWO',3:'ROT_THREE',4:'DUP_TOP',5:'DUP_TOP_TWO',9:'NOP',10:'UNARY_POSITIVE',11:'UNARY_NEGATIVE',12:'UNARY_NOT',15:'UNARY_INVERT',19:'BINARY_POWER',20:'BINARY_MULTIPLY',22:'BINARY_MODULO',23:'BINARY_ADD',24:'BINARY_SUBTRACT',25:'BINARY_SUBSCR',26:'BINARY_FLOOR_DIVIDE',27:'BINARY_TRUE_DIVIDE',28:'INPLACE_FLOOR_DIVIDE',29:'INPLACE_TRUE_DIVIDE',54:'STORE_MAP',55:'INPLACE_ADD',56:'INPLACE_SUBTRACT',57:'INPLACE_MULTIPLY',59:'INPLACE_MODULO',60:'STORE_SUBSCR',61:'DELETE_SUBSCR',62:'BINARY_LSHIFT',63:'BINARY_RSHIFT',64:'BINARY_AND',65:'BINARY_XOR',66:'BINARY_OR',67:'INPLACE_POWER',68:'GET_ITER',70:'PRINT_EXPR',71:'LOAD_BUILD_CLASS',72:'YIELD_FROM',75:'INPLACE_LSHIFT',76:'INPLACE_RSHIFT',77:'INPLACE_AND',78:'INPLACE_XOR',79:'INPLACE_OR',80:'BREAK_LOOP',81:'WITH_CLEANUP',83:'RETURN_VALUE',84:'IMPORT_STAR',86:'YIELD_VALUE',87:'POP_BLOCK',88:'END_FINALLY',89:'POP_EXCEPT',90:'STORE_NAME',91:'DELETE_NAME',92:'UNPACK_SEQUENCE',93:'FOR_ITER',94:'UNPACK_EX',95:'STORE_ATTR',96:'DELETE_ATTR',97:'STORE_GLOBAL',98:'DELETE_GLOBAL',100:'LOAD_CONST',101:'LOAD_NAME',102:'BUILD_TUPLE',103:'BUILD_LIST',104:'BUILD_SET',105:'BUILD_MAP',106:'LOAD_ATTR',107:'COMPARE_OP',108:'IMPORT_NAME',109:'IMPORT_FROM',110:'JUMP_FORWARD',111:'JUMP_IF_FALSE_OR_POP',112:'JUMP_IF_TRUE_OR_POP',113:'JUMP_ABSOLUTE',114:'POP_JUMP_IF_FALSE',115:'POP_JUMP_IF_TRUE',116:'LOAD_GLOBAL',119:'CONTINUE_LOOP',120:'SETUP_LOOP',121:'SETUP_EXCEPT',122:'SETUP_FINALLY',124:'LOAD_FAST',125:'STORE_FAST',126:'DELETE_FAST',130:'RAISE_VARARGS',131:'CALL_FUNCTION',132:'MAKE_FUNCTION',133:'BUILD_SLICE',134:'MAKE_CLOSURE',135:'LOAD_CLOSURE',136:'LOAD_DEREF',137:'STORE_DEREF',138:'DELETE_DEREF',140:'CALL_FUNCTION_VAR',141:'CALL_FUNCTION_KW',142:'CALL_FUNCTION_VAR_KW',143:'SETUP_WITH',144:'EXTENDED_ARG',145:'LIST_APPEND',146:'SET_ADD',147:'MAP_ADD',148:'LOAD_CLASSDEREF'}


def codes(value):
    if isinstance(value, dict) and value.get('kind') == 'code':
        yield value
        for c in value['consts']: yield from codes(c)


def display(code):
    print(f"\n### {code['name']} ({code['filename']}:{code['firstlineno']})")
    print('arguments:', code['varnames'][:code['argcount']])
    b, pos = code['code'], 0
    while pos < len(b):
        start, op = pos, b[pos]
        pos += 1
        arg, value = '', ''
        if op >= 90:
            arg = b[pos] | b[pos + 1] << 8
            pos += 2
            pool = code['consts'] if op == 100 else code['names'] if op in [90,91,95,96,97,98,101,106,108,109,116] else code['varnames'] if op in [124,125,126] else None
            if pool is not None:
                v = pool[arg]
                value = '<code ' + v['name'] + '>' if isinstance(v, dict) and v.get('kind') == 'code' else repr(v)
        print(f'{start:4} {OPS.get(op, str(op)):25} {str(arg):5} {value}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive')
    parser.add_argument('module')
    parser.add_argument('--function')
    args = parser.parse_args()
    with zipfile.ZipFile(args.archive) as archive:
        data = archive.read(args.module)
    # Python 3.4 uses a 12-byte pyc header.
    root = Reader(data[12:]).read()
    for code in codes(root):
        if args.function is None or code['name'] == args.function: display(code)
