import {useState} from 'react';
import {Check,FolderPlus,GitMerge,Pencil,Trash2,X} from 'lucide-react';
import {api} from './api';
import type {Collection} from './types';
type Props={collections:Collection[];onClose:()=>void;onRefresh:()=>Promise<void>;onError:(text:string)=>void};
const palette=['#91B9F7','#E1B36B','#AE9DEB','#E7959C','#70C4A4','#ABB9CD','#CBB6E3','#DBA58C'];
export default function CollectionsDialog({collections,onClose,onRefresh,onError}:Props){
 const [adding,setAdding]=useState(false);const [draft,setDraft]=useState('');
 const [edit,setEdit]=useState('');const [merging,setMerging]=useState('');const [target,setTarget]=useState('');
 const [busy,setBusy]=useState(false);
 async function run(fn:()=>Promise<unknown>){
  setBusy(true);try{await fn();await onRefresh();setAdding(false);setEdit('');setMerging('');setDraft('')}
  catch(e){onError(String(e))}finally{setBusy(false)}
 }
 function add(){if(draft.trim())run(()=>api.newCollection(draft.trim(),palette[collections.length%palette.length]))}
 function saveRename(){if(edit&&draft.trim())run(()=>api.renameCollection(edit,draft.trim()))}
 function merge(){if(merging&&target&&merging!==target)run(()=>api.mergeCollection(merging,target))}
 function remove(name:string){if(window.confirm('Delete collection "'+name+'"? Its meetings will move to Unsorted. No audio will be deleted.'))run(()=>api.deleteCollection(name))}
 return <div className="modal-backdrop" onMouseDown={e=>{if(e.currentTarget===e.target)onClose()}}><section className="concept-collection-modal" role="dialog" aria-modal="true" aria-labelledby="collection-title">
  <header><div><h2 id="collection-title">Manage collections</h2><p>Organize meetings by team, project or client. Removing a collection never deletes its recordings.</p></div><button className="icon-button" title="Close" aria-label="Close collection manager" onClick={onClose}><X size={20}/></button></header>
  <div className="concept-collection-rows">{collections.map(c=><div className="concept-collection-row" key={c.name}><div className="concept-collection-identity"><i style={{background:c.color}}/><strong>{c.name}</strong><small>{c.count} meetings</small></div><div className="concept-collection-operations">{c.name!=='Unsorted'&&<><button className="secondary" disabled={busy} onClick={()=>{setEdit(c.name);setDraft(c.name);setMerging('');setAdding(false)}}><Pencil size={15}/> Rename</button><button className="secondary" disabled={busy} onClick={()=>{setMerging(c.name);setTarget(collections.find(x=>x.name!==c.name)?.name||'');setEdit('');setAdding(false)}}><GitMerge size={15}/> Merge</button><button className="secondary danger-text" disabled={busy} onClick={()=>remove(c.name)}><Trash2 size={15}/> Delete</button></>}</div></div>)}</div>
  {adding||edit?<div className="concept-collection-editor"><label>{adding?'New collection':'Rename collection'}<input autoFocus placeholder="Collection name" value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'){if(adding)add();else saveRename()}}}/></label><button className="primary" disabled={busy||!draft.trim()} onClick={adding?add:saveRename}><Check size={16}/> Save</button><button className="secondary" onClick={()=>{setAdding(false);setEdit('')}}>Cancel</button></div>:null}
  {merging&&<div className="concept-collection-editor"><label>Merge {merging} into<select value={target} onChange={e=>setTarget(e.target.value)}>{collections.filter(c=>c.name!==merging).map(c=><option key={c.name} value={c.name}>{c.name}</option>)}</select></label><button className="primary" disabled={busy||!target} onClick={merge}>Merge collections</button><button className="secondary" onClick={()=>setMerging('')}>Cancel</button></div>}
  <footer><button className="secondary" onClick={()=>{setAdding(true);setEdit('');setMerging('');setDraft('')}}><FolderPlus size={17}/> New collection</button><button className="secondary" onClick={onClose}>Close</button></footer>
 </section></div>;
}
