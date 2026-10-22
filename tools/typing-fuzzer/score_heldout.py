import sys, collections
def score(path):
    rows=[l.rstrip('\n').split('\t') for l in open(path,encoding='utf-8')]
    h=rows[0]; ix={n:i for i,n in enumerate(h)}
    tot=ok=fp=miss=0; bycat=collections.defaultdict(lambda:[0,0,0,0])
    for r in rows[1:]:
        if len(r)<len(h): continue
        e=r[ix['expect']]; p=r[ix['predicted']]; cat=r[ix['category']]
        tot+=1; b=bycat[cat]; b[0]+=1
        if e==p: ok+=1; b[1]+=1
        elif e=='KEEP' and p=='CONVERT': fp+=1; b[2]+=1
        elif e=='CONVERT' and p=='KEEP': miss+=1; b[3]+=1
    print(f"{path.split('/')[-1]}: rows {tot}, accuracy {ok/tot*100:.2f}%, false {fp} ({fp/tot*100:.3f}%), missed {miss} ({miss/tot*100:.2f}%)")
    for cat,b in sorted(bycat.items()):
        print(f"   {cat:12s} n={b[0]:6d} acc={b[1]/b[0]*100:6.2f}% false={b[2]:4d} missed={b[3]:4d}")
for p in sys.argv[1:]: score(p)
