extern crate dbus;

use std::iter::FromIterator;
use std::collections::HashMap;

use dbus::{Connection, BusType, Message};
use dbus::arg::{Array, Dict, Variant, RefArg};

const XFCONF_BUS  : &str = "org.xfce.Xfconf";
const XFCONF_PATH : &str = "/org/xfce/Xfconf";
const XFCONF_OBJ  : &str = "org.xfce.Xfconf";
const XFCONF_DESKTOP_CHANNEL : &str = "xfce4-desktop";

struct XFCEDesktop {
    conn: Connection,
    // monitors: Vec<String>,
}

impl XFCEDesktop {
    fn new() -> Result<XFCEDesktop, dbus::Error> {
        let xfcedesktop = XFCEDesktop {
            conn: Connection::get_private(BusType::Session)?,
        };
        Ok(xfcedesktop)
    }

    fn call_func0(&self, method: &str) -> Result<Message, dbus::Error> {
        let m = Message::new_method_call(XFCONF_BUS, XFCONF_PATH, XFCONF_OBJ, method)
            .map_err(|emsg| dbus::Error::new_custom("new_method_call failed", emsg.as_str()))?;
        self.conn.send_with_reply_and_block(m, 2000)
    }

    fn call_func2(&self, method: &str, p1: &str, p2: &str) -> Result<Message, dbus::Error> {
        let m = Message::new_method_call(XFCONF_BUS, XFCONF_PATH, XFCONF_OBJ, method)
            .map_err(|emsg| dbus::Error::new_custom("new_method_call failed", emsg.as_str()))?.append2(p1, p2);
        self.conn.send_with_reply_and_block(m, 2000)
    }

    fn get_channels(&self) -> Result<Option<Vec<String>>,dbus::Error> {
        let m = self.call_func0("ListChannels")?;
        let arr: Array<&str,_> = m.get1().unwrap();
        Ok(Some(Vec::from_iter(arr.map(|x| x.to_owned()))))
    }

    fn prop_test(&self) -> Result<(),dbus::Error> {
        let m = self.call_func2("GetAllProperties", XFCONF_DESKTOP_CHANNEL, "/backdrop/screen0")?;
        let mut z: Dict<&str, Variant<Box<RefArg>>, _> = m.get1().unwrap();
        let mut keys = z.map(|(x,v)| x).collect::<Vec<&str>>();
        keys.sort();
        for k in keys {
            println!("{}", k);
        }
        Ok(())
    }
}

fn main() {
    let xd = XFCEDesktop::new().unwrap();
    xd.prop_test();
}
