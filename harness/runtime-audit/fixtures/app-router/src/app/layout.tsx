import {VStack,Box,globalCss} from '@devup-ui/react';
import {resetCss} from '@devup-ui/reset-css';
import {PersistentTabs} from '../components/PersistentTabs';
import {MotionController} from '../components/MotionController';
resetCss();
globalCss({body:{wordBreak:'keep-all',overflowWrap:'break-word'},'h1,p':{margin:0}});
export default function Layout({children}:{children:React.ReactNode}){
  return <html lang="ko"><body><VStack minH="100dvh"><header><PersistentTabs/></header><Box as="main" flex="1">{children}</Box><Box as="footer" bg="$surface" p="16px">Consumer footer</Box></VStack><MotionController bindings={[{kind:'sliding-route-highlight',target:'#consumer-tabs',links:'a',indicator:'#consumer-tab-indicator',durationMs:400}]}/></body></html>;
}
