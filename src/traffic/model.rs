#[cfg(test)]
use super::TrafficKind;

#[derive(Clone)]
pub(super) struct Record {
    pub(super) id: u64,
    pub(super) connection: Option<super::ConnectionTarget>,
    pub(super) cells: Vec<String>,
    pub(super) numbers: Vec<f64>,
    pub(super) active: bool,
    pub(super) enabled: bool,
    pub(super) details: String,
}
#[cfg(test)]
impl Record {
    fn new(id: u64, cells: &[&str]) -> Self {
        Self {
            id,
            connection: None,
            cells: cells.iter().map(|s| (*s).to_owned()).collect(),
            numbers: vec![],
            active: true,
            enabled: true,
            details: String::new(),
        }
    }
}
#[cfg(test)]
pub(super) fn samples(kind: TrafficKind) -> Vec<Record> {
    match kind {
        TrafficKind::Connections=>{
            let hosts=[("github.com","Safari","节点选择","DOMAIN-SUFFIX","64 KB/s","1.82 MB/s"),("rr1---sn.googlevideo.com","Safari","流媒体","RULE-SET","28 KB/s","986 KB/s"),("api.telegram.org","Telegram","节点选择","DOMAIN-SUFFIX","8 KB/s","42 KB/s"),("cdn.jsdelivr.net","Safari","自动选择","DOMAIN-SUFFIX","12 KB/s","168 KB/s"),("api.github.com","RustRover","节点选择","DOMAIN-SUFFIX","16 KB/s","92 KB/s"),("dns.alidns.com","Mihomo","DIRECT","IP-CIDR","2 KB/s","4 KB/s"),("apple.com","系统服务","DIRECT","GEOIP","1 KB/s","12 KB/s"),("fonts.googleapis.com","Safari","节点选择","DOMAIN-SUFFIX","4 KB/s","24 KB/s")];
            hosts.into_iter().enumerate().map(|(index,(host,process,chain,rule,up,down))|{
                let mut row=Record::new(index as u64+1,&[host, "18.6 MB", "2.4 MB", down, up, chain, rule, process, "18:42:06", "127.0.0.1:52341", "203.0.113.10:443", "TCP / HTTPS"]);
                row.numbers=vec![0.,18.6 * 1024. * 1024.,2.4 * 1024. * 1024.,[1864.,986.,42.,168.,92.,4.,12.,24.][index],[64.,28.,8.,12.,16.,2.,1.,4.][index],0.,0.,0.,67326.,0.,0.,0.];
                row.active=index<6;
                row.details=format!("目标主机\t{host}\n进程\t{process}\n代理链\t{chain} → 香港 01 · 专线\n规则\t{rule}\n网络\tTCP / HTTPS\n来源地址\t127.0.0.1:52341\n目标地址\t203.0.113.10:443\n上传速度\t{up}\n下载速度\t{down}\n上传总量\t2.4 MB\n下载总量\t18.6 MB\n开始时间\t18:42:06");row
            }).collect()
        },
        TrafficKind::Rules=>{
            [("DOMAIN-SUFFIX","github.com","节点选择","128 次","刚刚"),("RULE-SET","广告拦截","REJECT","1,246 次","2 秒前"),("DOMAIN-SUFFIX","google.com","节点选择","86 次","5 秒前"),("DOMAIN-KEYWORD","youtube","流媒体","64 次","12 秒前"),("GEOIP","CN","DIRECT","542 次","刚刚"),("IP-CIDR","192.168.0.0/16","DIRECT","216 次","1 分钟前"),("DOMAIN-SUFFIX","telegram.org","节点选择","38 次","2 分钟前"),("MATCH","所有其他流量","节点选择","172 次","刚刚")].into_iter().enumerate().map(|(i,(kind,payload,proxy,hits,last))|{
                let mut row=Record::new(i as u64+100,&[payload,kind,proxy,hits,last]);row.numbers=vec![0.,0.,0.,[128.,1246.,86.,64.,542.,216.,38.,172.][i],0.];
                row.details=format!("规则类型\t{kind}\n匹配内容\t{payload}\n出站策略\t{proxy}\n命中次数\t{hits}\n最近命中\t{last}");row
            }).collect()
        },
    }
}

pub(super) fn sorted_indices(
    records: &[Record],
    mut visible: Vec<usize>,
    column: usize,
    descending: bool,
) -> Vec<usize> {
    visible.sort_by(|a, b| {
        let a = &records[*a];
        let b = &records[*b];
        let order = if a.numbers.get(column).is_some_and(|n| *n != 0.)
            || b.numbers.get(column).is_some_and(|n| *n != 0.)
        {
            a.numbers
                .get(column)
                .unwrap_or(&0.)
                .total_cmp(b.numbers.get(column).unwrap_or(&0.))
        } else {
            a.cells.get(column).cmp(&b.cells.get(column))
        };
        if descending { order.reverse() } else { order }
    });
    visible
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_sort_uses_numeric_speed_and_preserves_identity() {
        let records = samples(TrafficKind::Connections);
        let indices = sorted_indices(&records, (0..records.len()).collect(), 3, true);
        assert_eq!(records[indices[0]].cells[0], "github.com");
        assert_eq!(records[indices[1]].cells[0], "rr1---sn.googlevideo.com");
    }
}
