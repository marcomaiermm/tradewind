#!/usr/bin/env python3
"""Regenerate deterministic synthetic compatibility fixtures (development only)."""
from pathlib import Path
import json,zipfile,stat
root=Path(__file__).resolve().parent.parent/'tests/fixtures'
root.mkdir(parents=True,exist_ok=True)
db='''-- Synthetic WoW SavedVariables fixture, no personal data.
TradeSkillMasterDB = {
 ["_scopeKeys"] = { ["realm"] = {[1]="Test Realm"}, ["factionrealm"] = {[1]="Alliance - Test Realm"} },
 ["_hash"] = 42,
 ["_version"] = 7,
 ["r@Test Realm@internalData@csvSales"] = "itemString,price,stackSize,quantity,time,source,buyer\\\\ni:42,100,1,2,90,Auction,Buyer\\\\ni:43,200,1,1,91,Auction,Other\\\\ni:44,300,1,1,92,Vendor,NPC",
 ["r@Test Realm@internalData@saveTimeSales"] = "100,101",
 ["r@Test Realm@internalData@csvBuys"] = "itemString,price,stackSize,quantity,time,source\\ni:42,80,1,2,80,Auction",
 ["r@Test Realm@internalData@saveTimeBuys"] = "102",
 ["r@Test Realm@internalData@csvExpired"] = "itemString,stackSize,quantity,time\\\\ni:42,1,1,85",
 ["r@Test Realm@internalData@saveTimeExpires"] = "103",
 ["r@Test Realm@internalData@csvCancelled"] = "itemString,stackSize,quantity,time\\\\ni:43,1,3,86",
 ["r@Test Realm@internalData@saveTimeCancels"] = "104",
 ["r@Test Realm@internalData@csvIncome"] = "type,amount,time\\\\nMail,12,87",
 ["r@Test Realm@internalData@csvExpense"] = "type,amount,time\\\\nRepair,13,88",
}
TSMItemInfoDB = { ["itemStrings"] = { [1] = "i:42\\002i:43\\002i:44" }, ["names"] = { [1] = "Test, Item\\002=Formula\\002Vendor Item" } }
'''
(root/'savedvariables.lua').write_text(db)
(root/'helper.lua').write_text('TradeSkillMaster_AppHelperDB = { region="EU-Classic", analytics={updateTime=123,data={"{\\\"event\\\":\\\"test\\\"}"}} }\n')
(root/'accounting-wire.json').write_text(json.dumps([[43,200,1,1,91,101,2],[42,80,1,2,80,102,3],[42,0,1,1,85,103,4],[43,0,1,3,86,104,5]],indent=2)+'\n')
(root/'accounting-sales.csv').write_bytes(b'itemString,itemName,price,stackSize,quantity,time,source,buyer\r\ni:42,"Test, Item",100,1,2,90,Auction,Buyer\r\ni:43,\'=Formula,200,1,1,91,Auction,Other\r\ni:44,Vendor Item,300,1,1,92,Vendor,NPC\r\n')
(root/'appdata.lua').write_text('select(2, ...).LoadData("AUCTIONDB_REGION_STAT","EU",[[return {value=42}]]) --<AUCTIONDB_REGION_STAT,EU,1700000000>\nselect(2, ...).LoadData("APP_INFO","Global",{version=41402,lastSync=1700000001}) --<APP_INFO,Global,1700000001>\n')
(root/'protocol.json').write_text(json.dumps({'version':41402,'time':1700000000,'signature':'ea86bf52b7be5e219b7985e645db45e01db8cbd6139e330fb107842d2528442f','realm':'Realm ü','encodedRealm':'UmVhbG0gw7w=','legacyApiUrl':'http://app-server.tradeskillmaster.com/v2/auth','oauthUrl':'https://id.tradeskillmaster.com/realms/app/protocol/openid-connect/token','cdnPriceUrl':'http://app-cdn.tradeskillmaster.xyz/fixture_gzip.txt?signature=fixture-only','authBody':{'token':'fixture-access-token'}},indent=2)+'\n')
entry={'name':'Test Realm','region':'EU','ahId':42,'appDataStrings':{'AUCTIONDB_NON_COMMODITY_DATA':{'lastModified':1700000000,'url':'https://data.tradeskillmaster.com/fixture_gzip.txt'}}}
status={'success':True,'channels':{'release':'Release','beta':'Beta'},'addons':[{'name':'TradeSkillMaster','version_str':'4.14.2'},{'name':'TradeSkillMaster_AppHelper','version_str':'4.14.2'}],'realms':[entry],'regions':[{'name':'EU','appDataStrings':{}}],'realms-Progression':[entry],'regions-Progression':[],'extraClassicRealms':[dict(entry,name='Test Realm-Alliance',region='Classic-EU')],'extraClassicRegions':[{'name':'Classic-EU','appDataStrings':{}}],'extraAnniversaryRealms':[dict(entry,name='Test Realm-Alliance',region='Anniversary-EU')],'extraAnniversaryRegions':[{'name':'Anniversary-EU','appDataStrings':{}}]}
(root/'status.json').write_text(json.dumps(status,indent=2)+'\n')
(root/'realms.json').write_text(json.dumps({'success':True,'retail':[{'id':42,'name':'Test Realm','region':'EU'}],'bcc':[{'id':'43','name':'Other Realm','region':'EU'}]},indent=2)+'\n')
(root/'cloud.json').write_text(json.dumps({'success':True,'data':{'abc_ACCOUNT-Anniversary':[{'timestamp':1700000000,'keep':True}]},'missing':['abc_ACCOUNT-Anniversary_1700000000']},indent=2)+'\n')
def archive(name, files, compression=zipfile.ZIP_DEFLATED):
 with zipfile.ZipFile(root/name,'w',compression=compression) as z:
  for path,content in files:
   info=zipfile.ZipInfo(path,date_time=(2023,11,14,22,13,20));info.compress_type=compression
   z.writestr(info,content)
archive('legacy-lzma.zip',[('TradeSkillMaster.lua',db),('TradeSkillMaster_AppHelper.lua',(root/'helper.lua').read_text())],zipfile.ZIP_LZMA)
archive('traversal.zip',[('../TradeSkillMaster.lua',db)])
archive('corrupt-lua.zip',[('TradeSkillMaster.lua','TradeSkillMasterDB = os.execute("bad")')])
archive('addon-helper.zip',[('TradeSkillMaster_AppHelper/TradeSkillMaster_AppHelper.toc','## Version: 4.14.2\n'),('TradeSkillMaster_AppHelper/AppData.lua','new data must not replace existing price cache\n')])
with zipfile.ZipFile(root/'symlink.zip','w') as z:
 info=zipfile.ZipInfo('TradeSkillMaster.lua');info.create_system=3;info.external_attr=(stat.S_IFLNK | 0o777)<<16;z.writestr(info,'/tmp/outside')
scan_csv='itemString,minBuyout,marketValue,numAuctions,quantity,lastScan\ni:42,100.9,120.8,2,3,1699999999\ni:43,200,220,4,5,1700000000\ni:44:1,300,320,6,7,1699999999'
scan_hash=5381
for byte in scan_csv.encode():
 scan_hash=(scan_hash*33+byte)%(1<<24)
(root/'scan.json').write_text(json.dumps({'f@Alliance - Test Realm@internalData@csvAuctionDBScan':scan_csv,'f@Alliance - Test Realm@internalData@auctionDBScanTime':1700000000,'f@Alliance - Test Realm@internalData@auctionDBScanHash':scan_hash},indent=2)+'\n')
(root/'scan-wire.json').write_text(json.dumps({'items':{'42':{'min_buyout':100,'market_value':120,'num_auctions':2,'quantity':3}},'last_modified':1700000000},indent=2)+'\n')
