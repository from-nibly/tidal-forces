import os,pathlib,tempfile,subprocess,shutil,time,select,json,re,sys
os.umask(0o077)
label=sys.argv[1];source=pathlib.Path(sys.argv[2]).resolve();root=pathlib.Path(tempfile.mkdtemp(prefix='tidal-picker-cpu-'+label+'-',dir='/tmp'))
exe=root/'tidal-forces';shutil.copy2(source,exe);exe.chmod(0o700)
env=os.environ.copy()
for key in list(env):
 if key.startswith(('PULSE_','TIDAL_')) or key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','HYPRLAND_INSTANCE_SIGNATURE','DISPLAY','DBUS_SESSION_BUS_ADDRESS','LD_PRELOAD','RUST_LOG','SSLKEYLOGFILE']:env.pop(key,None)
for key in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_CACHE_HOME','XDG_RUNTIME_DIR']:
 p=root/key.lower();p.mkdir(mode=0o700);env[key]=str(p)
alsa=root/'alsa.conf';alsa.write_text('pcm.!default { type null }\n')
env.update(ALSA_CONFIG_PATH=str(alsa),PULSE_SERVER='unix:'+str(root/'no-live-audio'),LIBGL_ALWAYS_SOFTWARE='1',LP_NUM_THREADS='1',XDG_SESSION_TYPE='x11',TMPDIR=str(root),DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system-bus'))
log=(root/'servers.log').open('w');reader,writer=os.pipe()
xvfb=subprocess.Popen(['Xvfb','-displayfd',str(writer),'-screen','0','1000x700x24','-nolisten','tcp'],env=env,pass_fds=(writer,),stdout=log,stderr=log);os.close(writer)
bus=None;results=[]
try:
 assert select.select([reader],[],[],10)[0];env['DISPLAY']=':'+str(int(os.read(reader,64).strip()))
 bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'],env=env,stdout=subprocess.PIPE,stderr=log,text=True)
 assert select.select([bus.stdout],[],[],5)[0];env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip();assert env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
 for mode in ['closed','empty','ready']:
  path=root/(mode+'.log')
  with path.open('w') as output:
   p=subprocess.Popen([str(exe),'--measure-cast-picker',mode,'8'],env=env,stdout=output,stderr=output)
   try:
    end=time.monotonic()+8
    while 'Synthetic idle probe ready' not in path.read_text():
     assert p.poll() is None and time.monotonic()<end,'Synthetic probe failed to initialize';time.sleep(.05)
    time.sleep(1.5)
    def ticks():
     values=pathlib.Path('/proc',str(p.pid),'stat').read_text().rsplit(')',1)[1].split();return int(values[11])+int(values[12])
    start=time.monotonic();before=ticks();time.sleep(3);after=ticks();elapsed=time.monotonic()-start
    cpu=(after-before)/os.sysconf('SC_CLK_TCK');code=p.wait(timeout=12);assert code==0,'Synthetic probe failed'
   finally:
    if p.poll() is None:p.terminate();p.wait(timeout=5)
  frames=int(re.search(r'Drawn frames: (\d+)',path.read_text())[1])
  result={'mode':mode,'cpu_seconds':round(cpu,3),'wall_seconds':round(elapsed,3),'cpu_percent':round(cpu/elapsed*100,2),'frames_in_8s':frames};results.append(result);print(json.dumps(result),flush=True)
finally:
 os.close(reader)
 if bus is not None:bus.terminate();bus.wait(timeout=5)
 xvfb.terminate();xvfb.wait(timeout=5);log.close()
(root/'results.json').write_text(json.dumps({'label':label,'scope':'Synthetic debug native UI, vsync off, privateXvfb/DBus/profile, inertaudio/backend, mockdiscovery, no LAN/account/device effects','results':results},indent=2));print('Private native evidence:',root)
