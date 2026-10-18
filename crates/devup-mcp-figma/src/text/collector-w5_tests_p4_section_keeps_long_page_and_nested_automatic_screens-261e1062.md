{TREE}
frame.height = 7240;
const wrapper = node('4:1', 'GROUP', 'Wrapper', frame);
node('5:1', 'FRAME', 'Screen', wrapper);
node('5:2', 'FRAME', 'Screen contents', wrapper.children[0]);
const small = node('6:1', 'FRAME', 'Case', section); small.width = small.height = 150;
node('6:2', 'TEXT', 'Text case', section);
