import sys, pyte
raw, cols, rows = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
sc = pyte.Screen(cols, rows); st = pyte.ByteStream(sc)
st.feed(open(raw, 'rb').read())
print("+" + "-"*cols + "+")
for line in sc.display: print("|" + line + "|")
print("+" + "-"*cols + "+")
