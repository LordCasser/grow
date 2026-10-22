import hashlib,importlib.util,json,os,subprocess,sys
from pathlib import Path
p=Path(__file__).resolve().parent;identity=json.loads((p/'identity.json').read_text());root=Path(identity['root'])
def clean_source():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==identity['commit']
 assert not subprocess.check_output(['git','status','--porcelain'],cwd=root).strip()
clean_source()
spec=importlib.util.spec_from_file_location('exact_runner',root/'scripts/run_exact_cargo_test.py');runner=importlib.util.module_from_spec(spec);sys.modules[spec.name]=runner;spec.loader.exec_module(runner)
original_phase=runner._run_phase;cache_path=p/'exact-compile-cache.json';cache=json.loads(cache_path.read_text()) if cache_path.exists() else {}
env_digest=hashlib.sha256(json.dumps(dict(os.environ),sort_keys=True).encode()).hexdigest()
def phase(command,name,*args,**kwargs):
 if name!='compile':return original_phase(command,name,*args,**kwargs)
 clean_source();key=hashlib.sha256(json.dumps([identity['commit'],env_digest,command]).encode()).hexdigest();row=cache.get(key)
 if row:
  executable=Path(row['executable']);assert executable.is_file();assert hashlib.sha256(executable.read_bytes()).hexdigest()==row['sha256']
  print('[compile] reuse verified executable from the same clean source, command and environment: '+str(executable),file=sys.stderr,flush=True)
  return runner.CommandOutput(returncode=0,test_executables={str(executable):row['cwd']})
 result=original_phase(command,name,*args,**kwargs)
 if result.returncode==0 and len(result.test_executables)==1:
  executable,cwd=next(iter(result.test_executables.items()));clean_source();cache[key]=dict(commit=identity['commit'],command=command,environment_sha256=env_digest,executable=executable,cwd=cwd,sha256=hashlib.sha256(Path(executable).read_bytes()).hexdigest());cache_path.write_text(json.dumps(cache,indent=2)+'\n')
 return result
runner._run_phase=phase
raise SystemExit(runner.main())
