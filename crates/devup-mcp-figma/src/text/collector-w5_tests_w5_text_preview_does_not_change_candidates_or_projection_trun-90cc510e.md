{TREE}
for (let i=0; i<60; i++) {{
 const f = node('4:'+i, 'FRAME', 'Screen '+i, section);
 const t = node('5:'+i, 'TEXT', 'Copy', f); t.characters = '한글 " preview '.repeat(40);
}}
