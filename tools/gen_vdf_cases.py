#!/usr/bin/env python3
"""Generate token-soup VDF texts with the verdict of the real python-vdf 3.4, for the ignored Rust test
`steam::vdf::tests::python_syntax_error_matches_python_vdf`:

    bench/.work/legacy/.venv/bin/python tools/gen_vdf_cases.py <seed> <count> cases.json
    PYVDF_CASES=cases.json cargo test -p patchops-core --lib python_syntax_error_matches_python_vdf -- --ignored
"""
random.seed(int(sys.argv[1]) if len(sys.argv)>1 else 1)
toks=['"a"','"b c"','{','}','{}','}}','//c','// "x"',' ','  ','\t','\n','\n','\n','\r\n','\r','abc','x y z','#x','"unterminated','\\','/','$','[$WIN]','""','"a\\"b"','"k"\t\t"v"','"k" "v"','"k"\n','"sec"\n{','{ }','"a" "b" "c"','"q" "multi','line"','"k\\\n','"v\\\n','\\n','a=b','<x>','-1','"x"{','"x" {}', '"x"{}', 'k  //c', 'k  /x', 'k /x/ y','\ufeff','\u00a0','\x0b','\x1c','é','"é"','@','(',"'",'?','*','.','a.b','%x', '=',' }','{ x']
cases=[]
for i in range(int(sys.argv[2]) if len(sys.argv)>2 else 5000):
    n=random.randint(1,14)
    t=''.join(random.choice(toks)+(random.choice(['','',' ','\n','\n']) ) for _ in range(n))
    cases.append(t)
out=[]
for t in cases:
    tt=t.replace('\r\n','\n').replace('\r','\n')
    try:
        vdf.parse(io.StringIO(tt)); out.append({"t":t,"exp":None})
    except SyntaxError as e:
        msg=e.msg.replace('vdf.parse: ',''); out.append({"t":t,"exp":[msg,e.args[1][1]]})
    except Exception as e:
        out.append({"t":t,"exp":["EXC "+type(e).__name__,0]})
json.dump(out,open(sys.argv[3],'w'))
print(len(out), sum(1 for o in out if o['exp'] is None), "accepted")
