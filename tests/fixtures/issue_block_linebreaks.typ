// #35: a block's source lines are one flowing paragraph in Typst — the HTML
// export splits its text at newlines, but the converter must collect the
// block as one paragraph, and recovery must not re-insert the paged line.
#block[ab
cd]

#block[aaa bbb
ccc ddd
eee fff ggg
hhh iii jjj]
